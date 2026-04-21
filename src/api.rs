use anyhow::Result;
use chrono::{DateTime, NaiveDateTime};
use log::info;
use regex::Regex;

use crate::models::{
    IntegrationPackage, LogEntry, ODataArtifactResponse, ODataPackageResponse, ODataResponse,
    RuntimeArtifact, TokenResponse,
};

const TOKEN_URL: &str = "https://crldevintegration.authentication.eu10.hana.ondemand.com/oauth/token?grant_type=client_credentials&token_format=jwt";
const PACKAGES_URL: &str =
    "https://crldevintegration.it-cpi001.cfapps.eu10.hana.ondemand.com/api/v1/IntegrationPackages";
const ARTIFACTS_URL: &str = "https://crldevintegration.it-cpi001.cfapps.eu10.hana.ondemand.com/api/v1/IntegrationRuntimeArtifacts";

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

    if let Ok(ms) = date_str.parse::<i64>() {
        return DateTime::from_timestamp(ms / 1000, ((ms % 1000) * 1_000_000) as u32)
            .map(|dt| dt.naive_utc());
    }

    if date_str.len() >= 19 {
        let iso_part = &date_str[0..19];
        return NaiveDateTime::parse_from_str(iso_part, "%Y-%m-%dT%H:%M:%S").ok();
    }

    None
}

pub async fn get_sap_token(client: &reqwest::Client) -> Result<String> {
    info!("Demande du jeton d'accès OAuth...");

    let client_id = std::env::var("CLIENT_ID")
        .expect("Variable CLIENT_ID introuvable (vérifie ton fichier .env)");
    let client_secret = std::env::var("CLIENT_SECRET")
        .expect("Variable CLIENT_SECRET introuvable (vérifie ton fichier .env)");

    let token_res = client
        .post(TOKEN_URL)
        .basic_auth(client_id, Some(client_secret))
        .send()
        .await?
        .error_for_status()?;

    let token_data: TokenResponse = token_res.json().await?;
    info!("Jeton obtenu !");

    Ok(token_data.access_token)
}

pub async fn fetch_sap_logs(
    client: &reqwest::Client,
    token: &str,
    top: u32,
    filter: Option<&str>,
) -> Result<Vec<LogEntry>> {
    let mut url = format!("{}&$top={}", "https://crldevintegration.it-cpi001.cfapps.eu10.hana.ondemand.com/api/v1/MessageProcessingLogs?$orderby=LogStart desc", top);

    if let Some(f) = filter {
        url.push_str(&format!("&$filter={}", f));
    }

    let res = client
        .get(&url)
        .bearer_auth(token)
        .header("Accept", "application/json")
        .send()
        .await?;

    if res.status().is_success() {
        let odata: ODataResponse = res.json().await?;
        let mut logs = odata.d.results;
        for log in &mut logs {
            log.parsed_date = clean_sap_date(log.log_start.as_deref());
            if log.status.as_deref() == Some("FAILED") {
                if let Some(artifact_id) = &log.integration_flow_name.clone() {
                    if let Ok(Some(err)) = fetch_artifact_error(client, token, artifact_id).await {
                        log.error_message = Some(err);
                    }
                }
            }
        }
        Ok(logs)
    } else {
        anyhow::bail!("❌ Erreur API SAP: {}", res.status());
    }
}

pub async fn fetch_packages(
    client: &reqwest::Client,
    token: &str,
) -> anyhow::Result<Vec<IntegrationPackage>> {
    log::info!("Téléchargement des Integration Packages...");
    let res = client
        .get(PACKAGES_URL)
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

pub async fn fetch_artifacts(
    client: &reqwest::Client,
    token: &str,
) -> anyhow::Result<Vec<RuntimeArtifact>> {
    log::info!("Téléchargement des Runtime Artifacts...");
    let res = client
        .get(ARTIFACTS_URL)
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

pub async fn fetch_artifact_error(
    client: &reqwest::Client,
    token: &str,
    artifact_id: &str,
) -> Result<Option<String>> {
    let error_url = format!(
        "https://crldevintegration.it-cpi001.cfapps.eu10.hana.ondemand.com/api/v1/IntegrationRuntimeArtifacts('{}')/ErrorInformation/$value",
        artifact_id
    );

    let res = client.get(&error_url).bearer_auth(token).send().await?;

    if res.status().is_success() {
        Ok(Some(res.text().await?))
    } else {
        Ok(None)
    }
}
