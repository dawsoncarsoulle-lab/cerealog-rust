use crate::config::{SapConfig, TokenCache};
use crate::models::{DesigntimeArtifact, ODataDesignResponse};
use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDateTime};
use futures::stream::{self, StreamExt};
use log::{info, warn};

use crate::models::{
    ArtifactConfiguration, IntegrationPackage, LogEntry, ODataArtifactResponse,
    ODataConfigResponse, ODataPackageResponse, ODataResponse, RuntimeArtifact, TokenResponse,
};

// ─── Parsing des dates SAP ────────────────────────────────────────────────────

fn clean_sap_date(raw_date: Option<&str>) -> Option<NaiveDateTime> {
    let date_str = raw_date?;

    // Format SAP historique: /Date(1714147200000)/
    if let Some(rest) = date_str.strip_prefix("/Date(") {
        if let Some(ms_str) = rest.split(')').next() {
            if let Ok(ms) = ms_str.parse::<i64>() {
                let secs = ms.div_euclid(1000);
                let nanos = ms.rem_euclid(1000) as u32 * 1_000_000;
                return DateTime::from_timestamp(secs, nanos).map(|dt| dt.naive_utc());
            }
        }
    }

    // Format ISO-like: 2026-04-28T08:00:00...
    if date_str.len() >= 19 {
        let iso_part = &date_str[0..19];
        return NaiveDateTime::parse_from_str(iso_part, "%Y-%m-%dT%H:%M:%S").ok();
    }

    None
}

// ─── HTTP client ─────────────────────────────────────────────────────────────

/// Client HTTP réutilisable avec pooling. Un seul client doit être partagé par le process.
pub fn build_http_client() -> Result<reqwest::Client> {
    build_http_client_with_options(200, 60)
}

pub fn build_http_client_with_options(
    pool_max_idle_per_host: usize,
    timeout_secs: u64,
) -> Result<reqwest::Client> {
    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(pool_max_idle_per_host)
        .pool_idle_timeout(std::time::Duration::from_secs(120))
        .tcp_keepalive(std::time::Duration::from_secs(60))
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()?;
    Ok(client)
}

// ─── Token OAuth ─────────────────────────────────────────────────────────────

pub async fn get_sap_token(client: &reqwest::Client, config: &SapConfig) -> Result<TokenCache> {
    info!("[{}] Demande du jeton OAuth", config.tenant_id);

    let token_res = client
        .post(&config.token_url)
        .basic_auth(&config.client_id, Some(&config.client_secret))
        .send()
        .await?
        .error_for_status()?;

    let token_data: TokenResponse = token_res.json().await?;
    Ok(TokenCache::new(token_data.access_token, 3500))
}

/// Renouvelle le token s'il est expiré en cours d'exécution.
pub async fn ensure_valid_token(
    client: &reqwest::Client,
    config: &SapConfig,
    cache: &mut TokenCache,
) -> Result<()> {
    if cache.is_expired() {
        let new_token = get_sap_token(client, config).await?;
        cache.refresh(new_token.token, 3500);
    }
    Ok(())
}

// ─── Logs ─────────────────────────────────────────────────────────────────────

#[allow(dead_code)]
pub async fn fetch_sap_logs(
    client: &reqwest::Client,
    token: &str,
    config: &SapConfig,
    top: u32,
    filter: Option<&str>,
    concurrency: usize,
) -> Result<Vec<LogEntry>> {
    fetch_sap_logs_paged(client, token, config, top, top, filter, concurrency, true).await
}

/// Récupération paginée des MPL.
///
/// `max_records` borne le volume total d'un cycle, `page_size` borne chaque requête OData.
/// Cela évite une réponse HTTP gigantesque et des batch SQL trop gros.
pub async fn fetch_sap_logs_paged(
    client: &reqwest::Client,
    token: &str,
    config: &SapConfig,
    max_records: u32,
    page_size: u32,
    filter: Option<&str>,
    error_concurrency: usize,
    fetch_error_details: bool,
) -> Result<Vec<LogEntry>> {
    let effective_page_size = page_size.clamp(1, 5_000);
    let mut skip = 0u32;
    let mut all_logs = Vec::with_capacity(max_records.min(effective_page_size) as usize);

    while skip < max_records {
        let wanted = (max_records - skip).min(effective_page_size);
        let url = config.logs_page_url(wanted, skip, filter);

        let res = client
            .get(&url)
            .bearer_auth(token)
            .header("Accept", "application/json")
            .send()
            .await
            .with_context(|| format!("[{}] requête logs échouée", config.tenant_id))?;

        if !res.status().is_success() {
            anyhow::bail!(
                "[{}] Erreur API SAP logs: HTTP {}",
                config.tenant_id,
                res.status()
            );
        }

        let odata: ODataResponse = res.json().await?;
        let fetched = odata.d.results.len() as u32;
        if fetched == 0 {
            break;
        }

        all_logs.extend(odata.d.results);
        if fetched < wanted {
            break;
        }
        skip = skip.saturating_add(fetched);
    }

    for log in &mut all_logs {
        log.parsed_date = clean_sap_date(log.log_start.as_deref());
    }

    if fetch_error_details {
        enrich_failed_logs_with_errors(client, token, config, &mut all_logs, error_concurrency)
            .await?;
    }

    Ok(all_logs)
}

async fn enrich_failed_logs_with_errors(
    client: &reqwest::Client,
    token: &str,
    config: &SapConfig,
    logs: &mut [LogEntry],
    concurrency: usize,
) -> Result<()> {
    let failed_guids: Vec<String> = logs
        .iter()
        .filter(|l| l.status.as_deref() == Some("FAILED"))
        .filter_map(|l| l.message_guid.clone())
        .collect();

    if failed_guids.is_empty() {
        return Ok(());
    }

    let error_results: Vec<(String, Option<String>)> = stream::iter(failed_guids.into_iter())
        .map(|guid| {
            let client = client.clone();
            let token = token.to_string();
            let error_url = config.log_error_url(&guid);
            async move {
                let err = fetch_error_from_url(&client, &token, &error_url)
                    .await
                    .unwrap_or_else(|e| {
                        warn!("Échec fetch erreur log {}: {}", guid, e);
                        None
                    });
                (guid, err)
            }
        })
        .buffer_unordered(concurrency.max(1))
        .collect()
        .await;

    let error_map: std::collections::HashMap<String, String> = error_results
        .into_iter()
        .filter_map(|(guid, err)| err.map(|e| (guid, e)))
        .collect();

    for log in logs {
        if let Some(guid) = &log.message_guid {
            if let Some(err) = error_map.get(guid) {
                log.error_message = Some(err.clone());
            }
        }
    }

    Ok(())
}

// ─── Packages ────────────────────────────────────────────────────────────────

pub async fn fetch_packages(
    client: &reqwest::Client,
    token: &str,
    config: &SapConfig,
) -> Result<Vec<IntegrationPackage>> {
    let res = client
        .get(&config.packages_url())
        .bearer_auth(token)
        .header("Accept", "application/json")
        .send()
        .await?;

    if res.status().is_success() {
        let odata: ODataPackageResponse = res.json().await?;
        let mut packages = odata.d.results;
        for pkg in &mut packages {
            pkg.parsed_creation_date = clean_sap_date(pkg.creation_date.as_deref());
        }
        Ok(packages)
    } else {
        anyhow::bail!("[{}] Échec Packages: HTTP {}", config.tenant_id, res.status());
    }
}

// ─── Artifacts runtime ───────────────────────────────────────────────────────

pub async fn fetch_artifacts(
    client: &reqwest::Client,
    token: &str,
    config: &SapConfig,
) -> Result<Vec<RuntimeArtifact>> {
    let res = client
        .get(&config.artifacts_url())
        .bearer_auth(token)
        .header("Accept", "application/json")
        .send()
        .await?;

    if res.status().is_success() {
        let odata: ODataArtifactResponse = res.json().await?;
        let mut artifacts = odata.d.results;
        for art in &mut artifacts {
            art.parsed_deployed_on = clean_sap_date(art.deployed_on.as_deref());
        }
        Ok(artifacts)
    } else {
        anyhow::bail!("[{}] Échec Artifacts: HTTP {}", config.tenant_id, res.status());
    }
}

// ─── Erreurs ─────────────────────────────────────────────────────────────────

async fn fetch_error_from_url(
    client: &reqwest::Client,
    token: &str,
    url: &str,
) -> Result<Option<String>> {
    let res = client.get(url).bearer_auth(token).send().await?;
    if res.status().is_success() {
        Ok(Some(res.text().await?))
    } else {
        Ok(None)
    }
}

pub async fn fetch_artifact_error(
    client: &reqwest::Client,
    token: &str,
    config: &SapConfig,
    artifact_id: &str,
) -> Result<Option<String>> {
    let url = config.artifact_error_url(artifact_id);
    fetch_error_from_url(client, token, &url).await
}

#[allow(dead_code)]
pub async fn fetch_log_error(
    client: &reqwest::Client,
    token: &str,
    config: &SapConfig,
    message_guid: &str,
) -> Result<Option<String>> {
    let url = config.log_error_url(message_guid);
    fetch_error_from_url(client, token, &url).await
}

// ─── Designtime artifacts ────────────────────────────────────────────────────

pub async fn fetch_artifacts_for_package(
    client: &reqwest::Client,
    token: &str,
    config: &SapConfig,
    package_id: &str,
) -> Result<Vec<DesigntimeArtifact>> {
    let url = config.package_artifacts_url(package_id);

    let res = client
        .get(&url)
        .bearer_auth(token)
        .header("Accept", "application/json")
        .send()
        .await?;

    if res.status().is_success() {
        let odata: ODataDesignResponse = res.json().await?;
        Ok(odata.d.results)
    } else {
        warn!(
            "[{}] Échec fetch artifacts pour package {}: HTTP {}",
            config.tenant_id,
            package_id,
            res.status()
        );
        Ok(vec![])
    }
}

pub async fn fetch_all_package_artifacts(
    client: &reqwest::Client,
    token: &str,
    config: &SapConfig,
    package_ids: &[String],
    concurrency: usize,
) -> std::collections::HashMap<String, Vec<String>> {
    let results: Vec<(String, Vec<String>)> = stream::iter(package_ids.iter().cloned())
        .map(|pkg_id| {
            let client = client.clone();
            let token = token.to_string();
            let cfg = config.clone();
            async move {
                let arts = fetch_artifacts_for_package(&client, &token, &cfg, &pkg_id)
                    .await
                    .unwrap_or_else(|e| {
                        warn!("Erreur fetch package {}: {}", pkg_id, e);
                        vec![]
                    });

                let ids = arts.into_iter().filter_map(|a| a.id).collect();
                (pkg_id, ids)
            }
        })
        .buffer_unordered(concurrency.max(1))
        .collect()
        .await;

    results.into_iter().collect()
}

// ─── Configurations ──────────────────────────────────────────────────────────

pub async fn fetch_artifact_properties(
    client: &reqwest::Client,
    token: &str,
    config: &SapConfig,
    artifact_id: &str,
) -> Result<Vec<ArtifactConfiguration>> {
    let url = config.artifact_configs_url(artifact_id);

    let res = client
        .get(&url)
        .bearer_auth(token)
        .header("Accept", "application/json")
        .send()
        .await?;

    if res.status().is_success() {
        let odata: ODataConfigResponse = res.json().await?;
        Ok(odata.d.results)
    } else {
        warn!(
            "[{}] Échec fetch configs pour artifact {}: HTTP {}",
            config.tenant_id,
            artifact_id,
            res.status()
        );
        Ok(vec![])
    }
}
