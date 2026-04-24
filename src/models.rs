use chrono::NaiveDateTime;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct TokenResponse {
    pub access_token: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct LogEntry {
    pub message_guid: Option<String>,
    pub status: Option<String>,
    pub log_start: Option<String>,
    pub integration_flow_name: Option<String>,

    #[serde(skip)]
    pub parsed_date: Option<NaiveDateTime>,

    #[serde(skip)]
    pub error_message: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct ODataResponse {
    pub d: ODataData,
}

#[derive(Deserialize, Debug)]
pub struct ODataData {
    pub results: Vec<LogEntry>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct IntegrationPackage {
    #[serde(alias = "Id", alias = "id")]
    pub id: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    pub vendor: Option<String>,
    #[serde(alias = "CreationDate", alias = "creationDate", alias = "Creationdate")]
    pub creation_date: Option<String>,

    pub industries: Option<String>,
    pub keywords: Option<String>,
    pub products: Option<String>,
    pub countries: Option<String>,
    #[serde(alias = "LineOfBusiness", alias = "lineOfBusiness")]
    pub line_of_business: Option<String>,

    #[serde(skip)]
    pub parsed_creation_date: Option<NaiveDateTime>,
}

#[derive(Deserialize, Debug)]
pub struct ODataPackageResponse {
    pub d: ODataPackageData,
}

#[derive(Deserialize, Debug)]
pub struct ODataPackageData {
    pub results: Vec<IntegrationPackage>,
}

#[derive(Deserialize, Debug, Clone, sqlx::FromRow)]
#[serde(rename_all = "PascalCase")]
pub struct RuntimeArtifact {
    #[serde(alias = "Id", alias = "id")]
    pub id: Option<String>,

    #[serde(
        alias = "PackageId",
        alias = "packageId",
        alias = "Packageid",
        alias = "packageid"
    )]
    pub package_id: Option<String>,

    pub name: Option<String>,
    pub version: Option<String>,

    #[serde(rename = "Type")]
    pub artifact_type: Option<String>,

    pub status: Option<String>,
    #[serde(alias = "DeployedOn", alias = "deployedOn", alias = "Deployedon")]
    pub deployed_on: Option<String>,

    #[serde(skip)]
    pub parsed_deployed_on: Option<NaiveDateTime>,
}

#[derive(Deserialize, Debug)]
pub struct ODataArtifactResponse {
    pub d: ODataArtifactData,
}

#[derive(Deserialize, Debug)]
pub struct ODataArtifactData {
    pub results: Vec<RuntimeArtifact>,
}

#[derive(Debug)]
pub struct ArtifactError {
    pub artifact_id: String,
    pub error_message: String,
    pub error_time: chrono::NaiveDateTime,
}

#[derive(Deserialize, Debug)]
pub struct ODataDesignResponse {
    pub d: ODataDesignData,
}

#[derive(Deserialize, Debug)]
pub struct ODataDesignData {
    pub results: Vec<DesigntimeArtifact>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct DesigntimeArtifact {
    #[serde(alias = "Id", alias = "id")]
    pub id: Option<String>,

    #[serde(alias = "PackageId", alias = "packageId")]
    pub package_id: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct ArtifactConfiguration {
    pub parameter_key: Option<String>,
    pub parameter_value: Option<String>,
    pub data_type: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct ODataConfigResponse {
    pub d: ODataConfigData,
}

#[derive(Deserialize, Debug)]
pub struct ODataConfigData {
    pub results: Vec<ArtifactConfiguration>,
}

// ─── Statistiques globales ───────────────────────────────────────────────────
// Défini ici (pas dans ui.rs) pour éviter les dépendances circulaires.

pub struct Stats {
    pub total_logs: i64,
    pub failed_logs: i64,
    pub total_packages: i64,
    pub total_artifacts: i64,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            total_logs: 0,
            failed_logs: 0,
            total_packages: 0,
            total_artifacts: 0,
        }
    }
}

// ─── Payload envoyé via le channel async worker → UI ────────────────────────

use crate::db::{ArtifactView, ErrorView, LogView, PackageView};
use std::collections::HashMap;

pub struct RefreshData {
    pub logs: Vec<LogView>,
    pub exec_errors: Vec<LogView>,
    pub artifacts: Vec<ArtifactView>,
    pub packages: Vec<PackageView>,
    pub deploy_errors: Vec<ErrorView>,
    pub configs: HashMap<String, Vec<(String, String)>>,
    pub stats: Stats,
    pub error_sparkline: Vec<u64>,
    pub error_barchart: Vec<(String, u64)>,
    pub activity_sparkline: Vec<u64>,
    pub top_errors_barchart: Vec<(String, u64)>,
    pub status_counts: Vec<(String, u64)>,
}
