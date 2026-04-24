use crate::config::{SapConfig, TokenCache, UserConfig};
use crate::models::{DesigntimeArtifact, ODataDesignResponse};
use anyhow::Result;
use chrono::{DateTime, NaiveDateTime};
use log::{info, warn};
use regex::Regex;

use crate::models::{
    ArtifactConfiguration, IntegrationPackage, LogEntry, ODataArtifactResponse,
    ODataConfigResponse, ODataPackageResponse, ODataResponse, RuntimeArtifact, TokenResponse,
};

// ─── Parsing des dates SAP ────────────────────────────────────────────────────

fn clean_sap_date(raw_date: Option<&str>) -> Option<NaiveDateTime> {
    let date_str = raw_date?;

    if let Ok(re) = Regex::new(r"/Date\((\d+)\)/") {
        if let Some(caps) = re.captures(date_str) {
            if let Ok(ms) = caps.get(1).unwrap().as_str().parse::<i64>() {
                return DateTime::from_timestamp(ms / 1000, ((ms % 1000) * 1_000_000) as u32)
                    .map(|dt| dt.naive_utc());
            }
        }
    }

    if date_str.len() >= 19 {
        let iso_part = &date_str[0..19];
        return NaiveDateTime::parse_from_str(iso_part, "%Y-%m-%dT%H:%M:%S").ok();
    }

    None
}

// ─── HTTP client ─────────────────────────────────────────────────────────────

/// Construit un client HTTP réutilisable avec connection pooling optimisé.
pub fn build_http_client() -> Result<reqwest::Client> {
    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(50)
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .tcp_keepalive(std::time::Duration::from_secs(60))
        .build()?;
    Ok(client)
}

// ─── Token OAuth ─────────────────────────────────────────────────────────────

pub async fn get_sap_token(client: &reqwest::Client, config: &SapConfig) -> Result<TokenCache> {
    info!("Demande du jeton d'accès OAuth...");

    let token_res = client
        .post(&config.token_url)
        .basic_auth(&config.client_id, Some(&config.client_secret))
        .send()
        .await?
        .error_for_status()?;

    let token_data: TokenResponse = token_res.json().await?;
    info!("Jeton obtenu !");

    // TTL par défaut de 3600s (1h), on renouvelle 60s avant
    Ok(TokenCache::new(token_data.access_token, 3500))
}

pub async fn get_or_refresh_token(
    client: &reqwest::Client,
    config: &SapConfig,
    user_config: &mut UserConfig,
) -> Result<TokenCache> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Token disque valide (marge 120s) ?
    if let (Some(tok), Some(exp)) = (
        &user_config.cached_token,
        user_config.cached_token_expires_at,
    ) {
        if exp > now + 120 {
            log::info!(
                "Token OAuth lu depuis le cache disque (expire dans {}s)",
                exp - now
            );
            let ttl = exp - now;
            return Ok(TokenCache::new(tok.clone(), ttl));
        }
    }

    // Sinon, fetch OAuth normal
    let cache = get_sap_token(client, config).await?;
    let expires_at = now + 3500;
    user_config.cached_token = Some(cache.get().to_string());
    user_config.cached_token_expires_at = Some(expires_at);
    user_config.save(); // Sauvegarde sur disque
    Ok(cache)
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

pub async fn fetch_sap_logs(
    client: &reqwest::Client,
    token: &str,
    config: &SapConfig,
    top: u32,
    filter: Option<&str>,
    concurrency: usize,
) -> Result<Vec<LogEntry>> {
    use futures::stream::{self, StreamExt};

    let url = config.logs_url(top, filter);

    let res = client
        .get(&url)
        .bearer_auth(token)
        .header("Accept", "application/json")
        .send()
        .await?;

    if res.status().is_success() {
        let odata: ODataResponse = res.json().await?;
        let mut logs = odata.d.results;

        let failed_guids: Vec<String> = logs
            .iter()
            .filter(|l| l.status.as_deref() == Some("FAILED"))
            .filter_map(|l| l.message_guid.clone())
            .collect();

        let error_stream = stream::iter(failed_guids.into_iter().map(|guid| {
            let client = client.clone();
            let token = token.to_string();
            let error_url = config.log_error_url(&guid);
            async move {
                let err = fetch_error_from_url(&client, &token, &error_url).await;
                (guid, err)
            }
        }));

        let error_results: Vec<_> = error_stream.buffer_unordered(concurrency).collect().await;

        let error_map: std::collections::HashMap<String, String> = error_results
            .into_iter()
            .filter_map(|(guid, res)| {
                res.unwrap_or_else(|e| {
                    warn!("Échec fetch erreur log {}: {}", guid, e);
                    None
                })
                .map(|e| (guid, e))
            })
            .collect();

        for log in &mut logs {
            log.parsed_date = clean_sap_date(log.log_start.as_deref());
            if let Some(guid) = &log.message_guid {
                if let Some(err) = error_map.get(guid) {
                    log.error_message = Some(err.clone());
                }
            }
        }

        Ok(logs)
    } else {
        anyhow::bail!("❌ Erreur API SAP logs: {}", res.status());
    }
}

// ─── Packages ────────────────────────────────────────────────────────────────

pub async fn fetch_packages(
    client: &reqwest::Client,
    token: &str,
    config: &SapConfig,
) -> Result<Vec<IntegrationPackage>> {
    log::info!("Téléchargement des Integration Packages...");
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
        log::info!("{} Packages trouvés.", packages.len());
        Ok(packages)
    } else {
        anyhow::bail!("Échec Packages: HTTP {}", res.status());
    }
}

// ─── Artifacts runtime ───────────────────────────────────────────────────────

pub async fn fetch_artifacts(
    client: &reqwest::Client,
    token: &str,
    config: &SapConfig,
) -> Result<Vec<RuntimeArtifact>> {
    log::info!("Téléchargement des Runtime Artifacts...");
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
        log::info!("{} Artifacts trouvés.", artifacts.len());
        Ok(artifacts)
    } else {
        anyhow::bail!("Échec Artifacts: HTTP {}", res.status());
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
            "Échec fetch artifacts pour package {}: HTTP {}",
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
    use std::sync::Arc;
    use tokio::sync::Semaphore;

    let sem = Arc::new(Semaphore::new(concurrency));
    let mut handles = Vec::new();

    for pkg_id in package_ids {
        let permit = sem.clone().acquire_owned().await.unwrap();

        let c = client.clone();
        let t = token.to_string();
        let p_id = pkg_id.clone();
        let cfg = config.clone();

        handles.push(tokio::spawn(async move {
            let arts = fetch_artifacts_for_package(&c, &t, &cfg, &p_id)
                .await
                .unwrap_or_else(|e| {
                    log::warn!("Erreur fetch package {}: {}", p_id, e);
                    vec![]
                });

            let mut ids = Vec::new();
            for a in arts {
                if let Some(id) = a.id {
                    ids.push(id);
                }
            }

            drop(permit);
            (p_id, ids)
        }));
    }

    let mut results = std::collections::HashMap::new();
    for h in handles {
        if let Ok((p_id, ids)) = h.await {
            results.insert(p_id, ids);
        }
    }

    results
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
            "Échec fetch configs pour artifact {}: HTTP {}",
            artifact_id,
            res.status()
        );
        Ok(vec![])
    }
}
