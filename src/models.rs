use chrono::NaiveDateTime;
use serde::Deserialize;
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
pub struct Tenant {
    pub id: String,
    pub name: String,
    pub client_name: Option<String>,
    pub shared_tenant: Option<bool>,
    pub active: Option<bool>,
    pub created_at: Option<NaiveDateTime>,
}

#[derive(Debug, Clone, Default, FromRow)]
pub struct OverviewStats {
    pub active_tenants: i64,
    pub total_logs: i64,
    pub failed_logs: i64,
    pub completed_logs: i64,
    pub total_packages: i64,
    pub total_artifacts: i64,
    pub pending_alerts: i64,
}

#[derive(Debug, Clone, FromRow)]
pub struct LogRow {
    pub tenant_id: String,
    pub parsed_date: Option<NaiveDateTime>,
    pub status: Option<String>,
    pub integration_flow_name: Option<String>,
    pub message_guid: String,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
pub struct PackageRow {
    pub tenant_id: String,
    pub id: String,
    pub name: Option<String>,
    pub version: Option<String>,
    pub vendor: Option<String>,
    pub creation_date: Option<NaiveDateTime>,
    pub tags: Option<String>,
    pub artifact_count: i64,
}

#[derive(Debug, Clone, FromRow)]
pub struct ArtifactRow {
    pub tenant_id: String,
    pub id: String,
    pub name: Option<String>,
    pub status: Option<String>,
    pub package_id: Option<String>,
    pub deployed_on: Option<NaiveDateTime>,
    pub artifact_type: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
pub struct ErrorRow {
    pub tenant_id: String,
    pub artifact_id: String,
    pub error_time: Option<NaiveDateTime>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
pub struct ConfigurationRow {
    pub tenant_id: String,
    pub artifact_id: String,
    pub parameter_key: String,
    pub parameter_value: Option<String>,
    pub data_type: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
pub struct PendingAlertRow {
    pub tenant_id: String,
    pub log_guid: String,
    pub flow_name: Option<String>,
    pub error_type: Option<String>,
    pub error_snippet: Option<String>,
    pub detected_at: Option<NaiveDateTime>,
}

#[derive(Debug, Clone, FromRow)]
pub struct SmartAlertRow {
    pub tenant_id: String,
    pub flow_name: String,
    pub alert_type: String,
    pub status: String,
    pub last_triggered_at: Option<NaiveDateTime>,
    pub extra: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct AlertData {
    pub pending: Vec<PendingAlertRow>,
    pub smart: Vec<SmartAlertRow>,
}

#[derive(Debug, Clone, Default)]
pub struct DataSet {
    pub tenants: Vec<Tenant>,
    pub stats: OverviewStats,
    pub recent_logs: Vec<LogRow>,
    pub logs: Vec<LogRow>,
    pub packages: Vec<PackageRow>,
    pub artifacts: Vec<ArtifactRow>,
    pub errors: Vec<ErrorRow>,
    pub configurations: Vec<ConfigurationRow>,
    pub alerts: AlertData,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CliConfig {
    pub tenant: Option<String>,
    pub limit: i64,
    pub refresh_seconds: u64,
}
