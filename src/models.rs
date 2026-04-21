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

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct IntegrationPackage {
    #[serde(alias = "Id", alias = "id")]
    pub id: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    pub vendor: Option<String>,
    #[serde(alias = "CreationDate", alias = "creationDate", alias = "Creationdate")]
    pub creation_date: Option<String>,

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
