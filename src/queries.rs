use anyhow::Result;
use sqlx::PgPool;

use crate::models::{
    AlertData, ArtifactRow, ConfigurationRow, DataSet, ErrorRow, LogRow, OverviewStats, PackageRow,
    PendingAlertRow, SmartAlertRow, Tenant,
};

const LEGACY_TENANT_ID: &str = "cerealog";

pub async fn load_dataset(
    pool: &PgPool,
    tenant: Option<&str>,
    search: Option<&str>,
    limit: i64,
) -> Result<DataSet> {
    let search = normalize_search(search);
    let (
        tenants,
        stats,
        recent_logs,
        logs,
        packages,
        artifacts,
        errors,
        configurations,
        pending_alerts,
        smart_alerts,
    ) = tokio::try_join!(
        fetch_active_tenants(pool),
        fetch_overview_stats(pool, tenant),
        fetch_recent_logs(pool, tenant, search.as_deref(), 10),
        fetch_logs(pool, tenant, search.as_deref(), limit),
        fetch_packages(pool, tenant, search.as_deref(), limit),
        fetch_artifacts(pool, tenant, search.as_deref(), limit),
        fetch_errors(pool, tenant, search.as_deref(), limit),
        fetch_configurations(pool, tenant, search.as_deref(), limit),
        fetch_pending_alerts(pool, tenant, search.as_deref(), limit),
        fetch_smart_alerts(pool, tenant, search.as_deref(), limit),
    )?;

    Ok(DataSet {
        tenants,
        stats,
        recent_logs,
        logs,
        packages,
        artifacts,
        errors,
        configurations,
        alerts: AlertData {
            pending: pending_alerts,
            smart: smart_alerts,
        },
    })
}

pub async fn fetch_active_tenants(pool: &PgPool) -> Result<Vec<Tenant>> {
    Ok(sqlx::query_as::<_, Tenant>(
        "SELECT id, name, client_name, shared_tenant, active, created_at
         FROM tenants
         WHERE active = true
         ORDER BY id",
    )
    .fetch_all(pool)
    .await?)
}

async fn fetch_overview_stats(pool: &PgPool, tenant: Option<&str>) -> Result<OverviewStats> {
    Ok(OverviewStats {
        active_tenants: count_active_tenants(pool, tenant).await?,
        total_logs: count_rows(pool, "sap_monitoring_logs", tenant, None).await?,
        failed_logs: count_rows(pool, "sap_monitoring_logs", tenant, Some("FAILED")).await?,
        completed_logs: count_rows(pool, "sap_monitoring_logs", tenant, Some("COMPLETED")).await?,
        total_packages: count_rows(pool, "integration_packages", tenant, None).await?,
        total_artifacts: count_rows(pool, "runtime_artifacts", tenant, None).await?,
        pending_alerts: count_rows(pool, "pending_alerts", tenant, None).await?,
    })
}

async fn fetch_recent_logs(
    pool: &PgPool,
    tenant: Option<&str>,
    search: Option<&str>,
    limit: i64,
) -> Result<Vec<LogRow>> {
    fetch_logs(pool, tenant, search, limit).await
}

async fn fetch_logs(
    pool: &PgPool,
    tenant: Option<&str>,
    search: Option<&str>,
    limit: i64,
) -> Result<Vec<LogRow>> {
    if !table_exists(pool, "sap_monitoring_logs").await? {
        return Ok(Vec::new());
    }

    let has_tenant = column_exists(pool, "sap_monitoring_logs", "tenant_id").await?;
    if !has_tenant && !legacy_tenant_matches(tenant) {
        return Ok(Vec::new());
    }

    if has_tenant {
        Ok(sqlx::query_as::<_, LogRow>(
            "SELECT tenant_id, parsed_date, status, integration_flow_name, message_guid, error_message
             FROM sap_monitoring_logs
             WHERE ($1::text IS NULL OR tenant_id = $1)
               AND ($2::text IS NULL
                    OR tenant_id ILIKE $2
                    OR status ILIKE $2
                    OR integration_flow_name ILIKE $2
                    OR message_guid ILIKE $2
                    OR error_message ILIKE $2)
             ORDER BY parsed_date DESC NULLS LAST
             LIMIT $3",
        )
        .bind(tenant)
        .bind(search)
        .bind(limit)
        .fetch_all(pool)
        .await?)
    } else {
        Ok(sqlx::query_as::<_, LogRow>(
            "SELECT 'cerealog'::text AS tenant_id, parsed_date, status, integration_flow_name, message_guid, error_message
             FROM sap_monitoring_logs
             WHERE ($1::text IS NULL
                    OR status ILIKE $1
                    OR integration_flow_name ILIKE $1
                    OR message_guid ILIKE $1
                    OR error_message ILIKE $1)
             ORDER BY parsed_date DESC NULLS LAST
             LIMIT $2",
        )
        .bind(search)
        .bind(limit)
        .fetch_all(pool)
        .await?)
    }
}

async fn fetch_packages(
    pool: &PgPool,
    tenant: Option<&str>,
    search: Option<&str>,
    limit: i64,
) -> Result<Vec<PackageRow>> {
    if !table_exists(pool, "integration_packages").await? {
        return Ok(Vec::new());
    }

    let packages_have_tenant = column_exists(pool, "integration_packages", "tenant_id").await?;
    let artifacts_have_tenant = column_exists(pool, "runtime_artifacts", "tenant_id").await?;
    let artifacts_exist = table_exists(pool, "runtime_artifacts").await?;

    if !packages_have_tenant && !legacy_tenant_matches(tenant) {
        return Ok(Vec::new());
    }

    match (packages_have_tenant, artifacts_exist, artifacts_have_tenant) {
        (true, true, true) => {
            Ok(sqlx::query_as::<_, PackageRow>(
                "SELECT p.tenant_id, p.id, p.name, p.version, p.vendor, p.creation_date, p.tags,
                        COUNT(a.id) AS artifact_count
                 FROM integration_packages p
                 LEFT JOIN runtime_artifacts a
                   ON a.tenant_id = p.tenant_id AND a.package_id = p.id
                 WHERE ($1::text IS NULL OR p.tenant_id = $1)
                   AND ($2::text IS NULL
                        OR p.tenant_id ILIKE $2
                        OR p.id ILIKE $2
                        OR p.name ILIKE $2
                        OR p.version ILIKE $2
                        OR p.vendor ILIKE $2
                        OR p.tags ILIKE $2)
                 GROUP BY p.tenant_id, p.id, p.name, p.version, p.vendor, p.creation_date, p.tags
                 ORDER BY p.name ASC NULLS LAST, p.id ASC
                 LIMIT $3",
            )
            .bind(tenant)
            .bind(search)
            .bind(limit)
            .fetch_all(pool)
            .await?)
        }
        (true, true, false) => {
            Ok(sqlx::query_as::<_, PackageRow>(
                "SELECT p.tenant_id, p.id, p.name, p.version, p.vendor, p.creation_date, p.tags,
                        COUNT(a.id) AS artifact_count
                 FROM integration_packages p
                 LEFT JOIN runtime_artifacts a ON a.package_id = p.id
                 WHERE ($1::text IS NULL OR p.tenant_id = $1)
                   AND ($2::text IS NULL
                        OR p.tenant_id ILIKE $2
                        OR p.id ILIKE $2
                        OR p.name ILIKE $2
                        OR p.version ILIKE $2
                        OR p.vendor ILIKE $2
                        OR p.tags ILIKE $2)
                 GROUP BY p.tenant_id, p.id, p.name, p.version, p.vendor, p.creation_date, p.tags
                 ORDER BY p.name ASC NULLS LAST, p.id ASC
                 LIMIT $3",
            )
            .bind(tenant)
            .bind(search)
            .bind(limit)
            .fetch_all(pool)
            .await?)
        }
        (false, true, _) => {
            Ok(sqlx::query_as::<_, PackageRow>(
                "SELECT 'cerealog'::text AS tenant_id, p.id, p.name, p.version, p.vendor, p.creation_date, p.tags,
                        COUNT(a.id) AS artifact_count
                 FROM integration_packages p
                 LEFT JOIN runtime_artifacts a ON a.package_id = p.id
                 WHERE ($1::text IS NULL
                        OR p.id ILIKE $1
                        OR p.name ILIKE $1
                        OR p.version ILIKE $1
                        OR p.vendor ILIKE $1
                        OR p.tags ILIKE $1)
                 GROUP BY p.id, p.name, p.version, p.vendor, p.creation_date, p.tags
                 ORDER BY p.name ASC NULLS LAST, p.id ASC
                 LIMIT $2",
            )
            .bind(search)
            .bind(limit)
            .fetch_all(pool)
            .await?)
        }
        (true, false, _) => {
            Ok(sqlx::query_as::<_, PackageRow>(
                "SELECT p.tenant_id, p.id, p.name, p.version, p.vendor, p.creation_date, p.tags,
                        0::bigint AS artifact_count
                 FROM integration_packages p
                 WHERE ($1::text IS NULL OR p.tenant_id = $1)
                   AND ($2::text IS NULL
                        OR p.tenant_id ILIKE $2
                        OR p.id ILIKE $2
                        OR p.name ILIKE $2
                        OR p.version ILIKE $2
                        OR p.vendor ILIKE $2
                        OR p.tags ILIKE $2)
                 ORDER BY p.name ASC NULLS LAST, p.id ASC
                 LIMIT $3",
            )
            .bind(tenant)
            .bind(search)
            .bind(limit)
            .fetch_all(pool)
            .await?)
        }
        (false, false, _) => {
            Ok(sqlx::query_as::<_, PackageRow>(
                "SELECT 'cerealog'::text AS tenant_id, p.id, p.name, p.version, p.vendor, p.creation_date, p.tags,
                        0::bigint AS artifact_count
                 FROM integration_packages p
                 WHERE ($1::text IS NULL
                        OR p.id ILIKE $1
                        OR p.name ILIKE $1
                        OR p.version ILIKE $1
                        OR p.vendor ILIKE $1
                        OR p.tags ILIKE $1)
                 ORDER BY p.name ASC NULLS LAST, p.id ASC
                 LIMIT $2",
            )
            .bind(search)
            .bind(limit)
            .fetch_all(pool)
            .await?)
        }
    }
}

async fn fetch_artifacts(
    pool: &PgPool,
    tenant: Option<&str>,
    search: Option<&str>,
    limit: i64,
) -> Result<Vec<ArtifactRow>> {
    if !table_exists(pool, "runtime_artifacts").await? {
        return Ok(Vec::new());
    }

    let has_tenant = column_exists(pool, "runtime_artifacts", "tenant_id").await?;
    let has_type = column_exists(pool, "runtime_artifacts", "artifact_type").await?;

    if !has_tenant && !legacy_tenant_matches(tenant) {
        return Ok(Vec::new());
    }

    match (has_tenant, has_type) {
        (true, true) => {
            Ok(sqlx::query_as::<_, ArtifactRow>(
                "SELECT tenant_id, id, name, status, package_id, deployed_on, artifact_type
                 FROM runtime_artifacts
                 WHERE ($1::text IS NULL OR tenant_id = $1)
                   AND ($2::text IS NULL
                        OR tenant_id ILIKE $2
                        OR id ILIKE $2
                        OR name ILIKE $2
                        OR status ILIKE $2
                        OR package_id ILIKE $2
                        OR artifact_type ILIKE $2)
                 ORDER BY deployed_on DESC NULLS LAST, name ASC NULLS LAST
                 LIMIT $3",
            )
            .bind(tenant)
            .bind(search)
            .bind(limit)
            .fetch_all(pool)
            .await?)
        }
        (true, false) => {
            Ok(sqlx::query_as::<_, ArtifactRow>(
                "SELECT tenant_id, id, name, status, package_id, deployed_on, NULL::text AS artifact_type
                 FROM runtime_artifacts
                 WHERE ($1::text IS NULL OR tenant_id = $1)
                   AND ($2::text IS NULL
                        OR tenant_id ILIKE $2
                        OR id ILIKE $2
                        OR name ILIKE $2
                        OR status ILIKE $2
                        OR package_id ILIKE $2)
                 ORDER BY deployed_on DESC NULLS LAST, name ASC NULLS LAST
                 LIMIT $3",
            )
            .bind(tenant)
            .bind(search)
            .bind(limit)
            .fetch_all(pool)
            .await?)
        }
        (false, true) => {
            Ok(sqlx::query_as::<_, ArtifactRow>(
                "SELECT 'cerealog'::text AS tenant_id, id, name, status, package_id, deployed_on, artifact_type
                 FROM runtime_artifacts
                 WHERE ($1::text IS NULL
                        OR id ILIKE $1
                        OR name ILIKE $1
                        OR status ILIKE $1
                        OR package_id ILIKE $1
                        OR artifact_type ILIKE $1)
                 ORDER BY deployed_on DESC NULLS LAST, name ASC NULLS LAST
                 LIMIT $2",
            )
            .bind(search)
            .bind(limit)
            .fetch_all(pool)
            .await?)
        }
        (false, false) => {
            Ok(sqlx::query_as::<_, ArtifactRow>(
                "SELECT 'cerealog'::text AS tenant_id, id, name, status, package_id, deployed_on, NULL::text AS artifact_type
                 FROM runtime_artifacts
                 WHERE ($1::text IS NULL
                        OR id ILIKE $1
                        OR name ILIKE $1
                        OR status ILIKE $1
                        OR package_id ILIKE $1)
                 ORDER BY deployed_on DESC NULLS LAST, name ASC NULLS LAST
                 LIMIT $2",
            )
            .bind(search)
            .bind(limit)
            .fetch_all(pool)
            .await?)
        }
    }
}

async fn fetch_errors(
    pool: &PgPool,
    tenant: Option<&str>,
    search: Option<&str>,
    limit: i64,
) -> Result<Vec<ErrorRow>> {
    if !table_exists(pool, "artifact_errors").await? {
        return Ok(Vec::new());
    }

    let has_tenant = column_exists(pool, "artifact_errors", "tenant_id").await?;
    if !has_tenant && !legacy_tenant_matches(tenant) {
        return Ok(Vec::new());
    }

    if has_tenant {
        Ok(sqlx::query_as::<_, ErrorRow>(
            "SELECT tenant_id, artifact_id, error_time, error_message
             FROM artifact_errors
             WHERE ($1::text IS NULL OR tenant_id = $1)
               AND ($2::text IS NULL
                    OR tenant_id ILIKE $2
                    OR artifact_id ILIKE $2
                    OR error_message ILIKE $2)
             ORDER BY error_time DESC NULLS LAST
             LIMIT $3",
        )
        .bind(tenant)
        .bind(search)
        .bind(limit)
        .fetch_all(pool)
        .await?)
    } else {
        Ok(sqlx::query_as::<_, ErrorRow>(
            "SELECT 'cerealog'::text AS tenant_id, artifact_id, error_time, error_message
             FROM artifact_errors
             WHERE ($1::text IS NULL
                    OR artifact_id ILIKE $1
                    OR error_message ILIKE $1)
             ORDER BY error_time DESC NULLS LAST
             LIMIT $2",
        )
        .bind(search)
        .bind(limit)
        .fetch_all(pool)
        .await?)
    }
}

async fn fetch_configurations(
    pool: &PgPool,
    tenant: Option<&str>,
    search: Option<&str>,
    limit: i64,
) -> Result<Vec<ConfigurationRow>> {
    if !table_exists(pool, "artifact_configurations").await? {
        return Ok(Vec::new());
    }

    let has_tenant = column_exists(pool, "artifact_configurations", "tenant_id").await?;
    if !has_tenant && !legacy_tenant_matches(tenant) {
        return Ok(Vec::new());
    }

    if has_tenant {
        Ok(sqlx::query_as::<_, ConfigurationRow>(
            "SELECT tenant_id, artifact_id, parameter_key, parameter_value, data_type
             FROM artifact_configurations
             WHERE ($1::text IS NULL OR tenant_id = $1)
               AND ($2::text IS NULL
                    OR tenant_id ILIKE $2
                    OR artifact_id ILIKE $2
                    OR parameter_key ILIKE $2
                    OR parameter_value ILIKE $2
                    OR data_type ILIKE $2)
             ORDER BY artifact_id ASC, parameter_key ASC
             LIMIT $3",
        )
        .bind(tenant)
        .bind(search)
        .bind(limit)
        .fetch_all(pool)
        .await?)
    } else {
        Ok(sqlx::query_as::<_, ConfigurationRow>(
            "SELECT 'cerealog'::text AS tenant_id, artifact_id, parameter_key, parameter_value, data_type
             FROM artifact_configurations
             WHERE ($1::text IS NULL
                    OR artifact_id ILIKE $1
                    OR parameter_key ILIKE $1
                    OR parameter_value ILIKE $1
                    OR data_type ILIKE $1)
             ORDER BY artifact_id ASC, parameter_key ASC
             LIMIT $2",
        )
        .bind(search)
        .bind(limit)
        .fetch_all(pool)
        .await?)
    }
}

async fn fetch_pending_alerts(
    pool: &PgPool,
    tenant: Option<&str>,
    search: Option<&str>,
    limit: i64,
) -> Result<Vec<PendingAlertRow>> {
    if !table_exists(pool, "pending_alerts").await? {
        return Ok(Vec::new());
    }

    let has_tenant = column_exists(pool, "pending_alerts", "tenant_id").await?;
    if !has_tenant && !legacy_tenant_matches(tenant) {
        return Ok(Vec::new());
    }

    if has_tenant {
        Ok(sqlx::query_as::<_, PendingAlertRow>(
            "SELECT tenant_id, log_guid, flow_name, error_type, error_snippet, detected_at
             FROM pending_alerts
             WHERE ($1::text IS NULL OR tenant_id = $1)
               AND ($2::text IS NULL
                    OR tenant_id ILIKE $2
                    OR log_guid ILIKE $2
                    OR flow_name ILIKE $2
                    OR error_type ILIKE $2
                    OR error_snippet ILIKE $2)
             ORDER BY detected_at DESC NULLS LAST
             LIMIT $3",
        )
        .bind(tenant)
        .bind(search)
        .bind(limit)
        .fetch_all(pool)
        .await?)
    } else {
        Ok(sqlx::query_as::<_, PendingAlertRow>(
            "SELECT 'cerealog'::text AS tenant_id, log_guid, flow_name, error_type, error_snippet, detected_at
             FROM pending_alerts
             WHERE ($1::text IS NULL
                    OR log_guid ILIKE $1
                    OR flow_name ILIKE $1
                    OR error_type ILIKE $1
                    OR error_snippet ILIKE $1)
             ORDER BY detected_at DESC NULLS LAST
             LIMIT $2",
        )
        .bind(search)
        .bind(limit)
        .fetch_all(pool)
        .await?)
    }
}

async fn fetch_smart_alerts(
    pool: &PgPool,
    tenant: Option<&str>,
    search: Option<&str>,
    limit: i64,
) -> Result<Vec<SmartAlertRow>> {
    if !table_exists(pool, "smart_alerts").await? {
        return Ok(Vec::new());
    }

    let has_tenant = column_exists(pool, "smart_alerts", "tenant_id").await?;
    if !has_tenant && !legacy_tenant_matches(tenant) {
        return Ok(Vec::new());
    }

    if has_tenant {
        Ok(sqlx::query_as::<_, SmartAlertRow>(
            "SELECT tenant_id, flow_name, alert_type, status, last_triggered_at, extra::text AS extra
             FROM smart_alerts
             WHERE ($1::text IS NULL OR tenant_id = $1)
               AND ($2::text IS NULL
                    OR tenant_id ILIKE $2
                    OR flow_name ILIKE $2
                    OR alert_type ILIKE $2
                    OR status ILIKE $2
                    OR extra::text ILIKE $2)
             ORDER BY last_triggered_at DESC NULLS LAST
             LIMIT $3",
        )
        .bind(tenant)
        .bind(search)
        .bind(limit)
        .fetch_all(pool)
        .await?)
    } else {
        Ok(sqlx::query_as::<_, SmartAlertRow>(
            "SELECT 'cerealog'::text AS tenant_id, flow_name, alert_type, status, last_triggered_at, extra::text AS extra
             FROM smart_alerts
             WHERE ($1::text IS NULL
                    OR flow_name ILIKE $1
                    OR alert_type ILIKE $1
                    OR status ILIKE $1
                    OR extra::text ILIKE $1)
             ORDER BY last_triggered_at DESC NULLS LAST
             LIMIT $2",
        )
        .bind(search)
        .bind(limit)
        .fetch_all(pool)
        .await?)
    }
}

async fn count_active_tenants(pool: &PgPool, tenant: Option<&str>) -> Result<i64> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)
         FROM tenants
         WHERE active = true AND ($1::text IS NULL OR id = $1)",
    )
    .bind(tenant)
    .fetch_one(pool)
    .await?)
}

async fn count_rows(
    pool: &PgPool,
    table_name: &str,
    tenant: Option<&str>,
    status: Option<&str>,
) -> Result<i64> {
    if !table_exists(pool, table_name).await? {
        return Ok(0);
    }

    let has_tenant = column_exists(pool, table_name, "tenant_id").await?;
    if !has_tenant && !legacy_tenant_matches(tenant) {
        return Ok(0);
    }

    match (table_name, has_tenant, status) {
        ("sap_monitoring_logs", true, Some(status)) => Ok(sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sap_monitoring_logs WHERE status = $1 AND ($2::text IS NULL OR tenant_id = $2)",
        )
        .bind(status)
        .bind(tenant)
        .fetch_one(pool)
        .await?),
        ("sap_monitoring_logs", false, Some(status)) => Ok(sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sap_monitoring_logs WHERE status = $1",
        )
        .bind(status)
        .fetch_one(pool)
        .await?),
        ("sap_monitoring_logs", true, None) => Ok(sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sap_monitoring_logs WHERE ($1::text IS NULL OR tenant_id = $1)",
        )
        .bind(tenant)
        .fetch_one(pool)
        .await?),
        ("sap_monitoring_logs", false, None) => Ok(sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sap_monitoring_logs",
        )
        .fetch_one(pool)
        .await?),
        ("integration_packages", true, _) => Ok(sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM integration_packages WHERE ($1::text IS NULL OR tenant_id = $1)",
        )
        .bind(tenant)
        .fetch_one(pool)
        .await?),
        ("integration_packages", false, _) => Ok(sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM integration_packages",
        )
        .fetch_one(pool)
        .await?),
        ("runtime_artifacts", true, _) => Ok(sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM runtime_artifacts WHERE ($1::text IS NULL OR tenant_id = $1)",
        )
        .bind(tenant)
        .fetch_one(pool)
        .await?),
        ("runtime_artifacts", false, _) => Ok(sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM runtime_artifacts",
        )
        .fetch_one(pool)
        .await?),
        ("pending_alerts", true, _) => Ok(sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM pending_alerts WHERE ($1::text IS NULL OR tenant_id = $1)",
        )
        .bind(tenant)
        .fetch_one(pool)
        .await?),
        ("pending_alerts", false, _) => Ok(sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM pending_alerts",
        )
        .fetch_one(pool)
        .await?),
        _ => Ok(0),
    }
}

fn legacy_tenant_matches(tenant: Option<&str>) -> bool {
    tenant
        .map(|tenant| tenant == LEGACY_TENANT_ID)
        .unwrap_or(true)
}

fn normalize_search(search: Option<&str>) -> Option<String> {
    search
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| format!("%{}%", s))
}

async fn table_exists(pool: &PgPool, table_name: &str) -> Result<bool> {
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1
             FROM information_schema.tables
             WHERE table_schema = current_schema()
               AND table_name = $1
         )",
    )
    .bind(table_name)
    .fetch_one(pool)
    .await?)
}

async fn column_exists(pool: &PgPool, table_name: &str, column_name: &str) -> Result<bool> {
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1
             FROM information_schema.columns
             WHERE table_schema = current_schema()
               AND table_name = $1
               AND column_name = $2
         )",
    )
    .bind(table_name)
    .bind(column_name)
    .fetch_one(pool)
    .await?)
}
