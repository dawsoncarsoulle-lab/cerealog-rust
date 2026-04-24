use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, Instant};

// ─── Configuration SAP (depuis les variables d'environnement) ────────────────

#[derive(Debug, Clone)]
pub struct SapConfig {
    pub base_url: String,
    pub token_url: String,
    pub client_id: String,
    pub client_secret: String,
}

impl SapConfig {
    pub fn from_env() -> Result<Self> {
        let base_url = std::env::var("SAP_BASE_URL").context(
            "Variable SAP_BASE_URL manquante. Exemple : https://xxx.it-cpi001.cfapps.eu10.hana.ondemand.com",
        )?;
        let token_url = std::env::var("SAP_TOKEN_URL").context(
            "Variable SAP_TOKEN_URL manquante. Exemple : https://xxx.authentication.eu10.hana.ondemand.com/oauth/token?grant_type=client_credentials&token_format=jwt",
        )?;
        let client_id = std::env::var("CLIENT_ID")
            .context("Variable CLIENT_ID manquante (vérifie ton fichier .env)")?;
        let client_secret = std::env::var("CLIENT_SECRET")
            .context("Variable CLIENT_SECRET manquante (vérifie ton fichier .env)")?;

        Ok(Self {
            base_url,
            token_url,
            client_id,
            client_secret,
        })
    }

    pub fn packages_url(&self) -> String {
        format!("{}/api/v1/IntegrationPackages", self.base_url)
    }

    pub fn artifacts_url(&self) -> String {
        format!("{}/api/v1/IntegrationRuntimeArtifacts", self.base_url)
    }

    pub fn logs_url(&self, top: u32, filter: Option<&str>) -> String {
        let mut url = format!(
                "{}/api/v1/MessageProcessingLogs?$select=MessageGuid,Status,LogStart,IntegrationFlowName&$orderby=LogStart desc&$top={}",
                self.base_url, top
            );
        if let Some(f) = filter {
            url.push_str(&format!("&$filter={}", f));
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

// ─── Cache de token OAuth avec gestion d'expiration ─────────────────────────

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
        // Renouvelle 60s avant l'expiration réelle
        Instant::now() >= self.expires_at - Duration::from_secs(60)
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

// ─── Configuration utilisateur persistée ────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserConfig {
    pub logs_limit: u32,
    pub parallel_requests: usize,
    #[serde(default)]
    pub cached_token: Option<String>,
    #[serde(default)]
    pub cached_token_expires_at: Option<u64>,
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            logs_limit: 200,
            parallel_requests: 20,
            cached_token: None,
            cached_token_expires_at: None,
        }
    }
}

impl UserConfig {
    fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|p| p.join("sap-extractor").join("config.toml"))
    }

    pub fn load() -> Self {
        let Some(path) = Self::config_path() else {
            return Self::default();
        };

        match std::fs::read_to_string(&path) {
            Ok(content) => toml::from_str(&content).unwrap_or_else(|e| {
                log::warn!(
                    "Config utilisateur invalide ({}) — utilisation des défauts",
                    e
                );
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let Some(path) = Self::config_path() else {
            return;
        };

        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                log::warn!("Impossible de créer le dossier config: {}", e);
                return;
            }
        }

        match toml::to_string_pretty(self) {
            Ok(content) => {
                if let Err(e) = std::fs::write(&path, content) {
                    log::warn!("Impossible de sauvegarder la config: {}", e);
                }
            }
            Err(e) => log::warn!("Sérialisation config échouée: {}", e),
        }
    }
}
