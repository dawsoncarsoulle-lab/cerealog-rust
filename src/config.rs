use anyhow::{Context, Result};
use std::time::{Duration, Instant};

// ─── Configuration SAP ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SapConfig {
    pub tenant_id: String,
    pub base_url: String,
    pub token_url: String,
    pub client_id: String,
    pub client_secret: String,
}

impl SapConfig {
    /// Fallback .env utile pour des tests locaux isolés.
    #[allow(dead_code)]
    pub fn from_env() -> Result<Self> {
        let base_url = std::env::var("SAP_BASE_URL").context("Variable SAP_BASE_URL manquante.")?;
        let token_url =
            std::env::var("SAP_TOKEN_URL").context("Variable SAP_TOKEN_URL manquante.")?;
        let client_id = std::env::var("CLIENT_ID").context("Variable CLIENT_ID manquante")?;
        let client_secret =
            std::env::var("CLIENT_SECRET").context("Variable CLIENT_SECRET manquante")?;

        Ok(Self {
            tenant_id: std::env::var("TENANT_ID").unwrap_or_else(|_| "cerealog".to_string()),
            base_url,
            token_url,
            client_id,
            client_secret,
        })
    }

    /// Charge tous les tenants actifs depuis PostgreSQL.
    pub async fn load_all_from_db(pool: &sqlx::PgPool) -> Result<Vec<Self>> {
        let rows: Vec<(String, String, String, String, String)> = sqlx::query_as(
            "SELECT id, sap_base_url, sap_token_url, sap_client_id, sap_client_secret_enc
             FROM tenants
             WHERE active = true
               AND sap_base_url IS NOT NULL
               AND sap_token_url IS NOT NULL
               AND sap_client_id IS NOT NULL
               AND sap_client_secret_enc IS NOT NULL",
        )
        .fetch_all(pool)
        .await?;

        rows.into_iter()
            .map(|(id, base_url, token_url, client_id, secret_enc)| {
                let client_secret = crate::crypto::decrypt(&secret_enc)?;
                Ok(Self {
                    tenant_id: id,
                    base_url: base_url.trim_end_matches('/').to_string(),
                    token_url,
                    client_id,
                    client_secret,
                })
            })
            .collect()
    }

    pub fn packages_url(&self) -> String {
        format!("{}/api/v1/IntegrationPackages", self.base_url)
    }

    pub fn artifacts_url(&self) -> String {
        format!("{}/api/v1/IntegrationRuntimeArtifacts", self.base_url)
    }

    pub fn logs_url(&self, top: u32, filter: Option<&str>) -> String {
        self.logs_page_url(top, 0, filter)
    }

    /// URL paginée pour absorber de gros volumes sans charger un `$top` massif.
    /// SAP CPI OData supporte `$top` + `$skip` sur MessageProcessingLogs.
    pub fn logs_page_url(&self, top: u32, skip: u32, filter: Option<&str>) -> String {
        let mut url = format!(
            "{}/api/v1/MessageProcessingLogs?$select=MessageGuid,Status,LogStart,IntegrationFlowName&$orderby=LogStart desc&$top={}&$skip={}",
            self.base_url, top, skip
        );
        if let Some(f) = filter {
            url.push_str("&$filter=");
            url.push_str(&encode_query_component(f));
        }
        url
    }

    pub fn log_error_url(&self, message_guid: &str) -> String {
        format!(
            "{}/api/v1/MessageProcessingLogs('{}')/ErrorInformation/$value",
            self.base_url, message_guid
        )
    }

    pub fn artifact_error_url(&self, artifact_id: &str) -> String {
        format!(
            "{}/api/v1/IntegrationRuntimeArtifacts('{}')/ErrorInformation/$value",
            self.base_url, artifact_id
        )
    }

    pub fn package_artifacts_url(&self, package_id: &str) -> String {
        format!(
            "{}/api/v1/IntegrationPackages('{}')/IntegrationDesigntimeArtifacts",
            self.base_url, package_id
        )
    }

    pub fn artifact_configs_url(&self, artifact_id: &str) -> String {
        format!(
            "{}/api/v1/IntegrationDesigntimeArtifacts(Id='{}',Version='active')/Configurations",
            self.base_url, artifact_id
        )
    }
}

fn encode_query_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

// ─── Cache de token OAuth en mémoire ─────────────────────────────────────────

pub struct TokenCache {
    pub token: String,
    expires_at: Instant,
}

impl TokenCache {
    pub fn new(token: String, ttl_secs: u64) -> Self {
        Self {
            token,
            expires_at: Instant::now() + Duration::from_secs(ttl_secs),
        }
    }

    pub fn is_expired(&self) -> bool {
        Instant::now() + Duration::from_secs(60) >= self.expires_at
    }

    pub fn get(&self) -> &str {
        &self.token
    }

    pub fn refresh(&mut self, new_token: String, ttl_secs: u64) {
        self.token = new_token;
        self.expires_at = Instant::now() + Duration::from_secs(ttl_secs);
        log::info!("Token OAuth renouvelé (expire dans {}s)", ttl_secs);
    }
}
