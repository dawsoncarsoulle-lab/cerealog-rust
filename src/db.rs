use crate::models::{ArtifactError, IntegrationPackage, LogEntry, RuntimeArtifact};
use anyhow::Result;
use serde_json;
use sqlx::{Postgres, QueryBuilder};

const LOG_INSERT_BATCH_SIZE: usize = 5_000;
const PACKAGE_INSERT_BATCH_SIZE: usize = 2_000;
const ARTIFACT_INSERT_BATCH_SIZE: usize = 2_000;
const MAPPING_UPDATE_BATCH_SIZE: usize = 10_000;

// ─── Insertions batchées ─────────────────────────────────────────────────────

pub async fn insert_logs(pool: &sqlx::PgPool, tenant_id: &str, logs: Vec<LogEntry>) -> Result<()> {
    if logs.is_empty() {
        return Ok(());
    }

    let mut batch = Vec::with_capacity(LOG_INSERT_BATCH_SIZE);
    for log in logs {
        batch.push(log);
        if batch.len() >= LOG_INSERT_BATCH_SIZE {
            insert_logs_batch(pool, tenant_id, std::mem::take(&mut batch)).await?;
        }
    }
    if !batch.is_empty() {
        insert_logs_batch(pool, tenant_id, batch).await?;
    }
    Ok(())
}

async fn insert_logs_batch(
    pool: &sqlx::PgPool,
    tenant_id: &str,
    logs: Vec<LogEntry>,
) -> Result<()> {
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
        "INSERT INTO sap_monitoring_logs \
         (message_guid, status, parsed_date, error_message, integration_flow_name, tenant_id) ",
    );

    qb.push_values(logs, |mut b, log| {
        b.push_bind(log.message_guid)
            .push_bind(log.status)
            .push_bind(log.parsed_date)
            .push_bind(log.error_message)
            .push_bind(log.integration_flow_name)
            .push_bind(tenant_id);
    });

    qb.push(
        " ON CONFLICT (message_guid) DO UPDATE SET \
         status = EXCLUDED.status, \
         parsed_date = EXCLUDED.parsed_date, \
         error_message = EXCLUDED.error_message, \
         integration_flow_name = EXCLUDED.integration_flow_name, \
         tenant_id = EXCLUDED.tenant_id",
    );

    qb.build().execute(pool).await?;
    Ok(())
}

pub async fn insert_packages(
    pool: &sqlx::PgPool,
    tenant_id: &str,
    packages: Vec<IntegrationPackage>,
) -> Result<()> {
    if packages.is_empty() {
        return Ok(());
    }

    let mut batch = Vec::with_capacity(PACKAGE_INSERT_BATCH_SIZE);
    for pkg in packages {
        batch.push(pkg);
        if batch.len() >= PACKAGE_INSERT_BATCH_SIZE {
            insert_packages_batch(pool, tenant_id, std::mem::take(&mut batch)).await?;
        }
    }
    if !batch.is_empty() {
        insert_packages_batch(pool, tenant_id, batch).await?;
    }
    Ok(())
}

async fn insert_packages_batch(
    pool: &sqlx::PgPool,
    tenant_id: &str,
    packages: Vec<IntegrationPackage>,
) -> Result<()> {
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
        "INSERT INTO integration_packages (id, name, version, vendor, creation_date, tags, tenant_id) ",
    );

    qb.push_values(packages, |mut b, pkg| {
        let mut all_tags = Vec::new();
        if let Some(val) = &pkg.industries {
            if !val.is_empty() {
                all_tags.push(val.clone());
            }
        }
        if let Some(val) = &pkg.keywords {
            if !val.is_empty() {
                all_tags.push(val.clone());
            }
        }
        if let Some(val) = &pkg.products {
            if !val.is_empty() {
                all_tags.push(val.clone());
            }
        }
        if let Some(val) = &pkg.countries {
            if !val.is_empty() {
                all_tags.push(val.clone());
            }
        }
        if let Some(val) = &pkg.line_of_business {
            if !val.is_empty() {
                all_tags.push(val.clone());
            }
        }

        let final_tags = if all_tags.is_empty() {
            None
        } else {
            Some(all_tags.join(", "))
        };

        b.push_bind(pkg.id)
            .push_bind(pkg.name)
            .push_bind(pkg.version)
            .push_bind(pkg.vendor)
            .push_bind(pkg.parsed_creation_date)
            .push_bind(final_tags)
            .push_bind(tenant_id);
    });

    qb.push(
        " ON CONFLICT (id) DO UPDATE SET \
             name = EXCLUDED.name, \
             version = EXCLUDED.version, \
             vendor = EXCLUDED.vendor, \
             creation_date = EXCLUDED.creation_date, \
             tags = EXCLUDED.tags, \
             tenant_id = EXCLUDED.tenant_id",
    );

    qb.build().execute(pool).await?;
    Ok(())
}

pub async fn insert_artifacts(
    pool: &sqlx::PgPool,
    tenant_id: &str,
    artifacts: Vec<RuntimeArtifact>,
) -> Result<()> {
    if artifacts.is_empty() {
        return Ok(());
    }

    let mut batch = Vec::with_capacity(ARTIFACT_INSERT_BATCH_SIZE);
    for art in artifacts {
        batch.push(art);
        if batch.len() >= ARTIFACT_INSERT_BATCH_SIZE {
            insert_artifacts_batch(pool, tenant_id, std::mem::take(&mut batch)).await?;
        }
    }
    if !batch.is_empty() {
        insert_artifacts_batch(pool, tenant_id, batch).await?;
    }
    Ok(())
}

async fn insert_artifacts_batch(
    pool: &sqlx::PgPool,
    tenant_id: &str,
    artifacts: Vec<RuntimeArtifact>,
) -> Result<()> {
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
        "INSERT INTO runtime_artifacts (id, name, status, deployed_on, package_id, tenant_id) ",
    );

    qb.push_values(artifacts, |mut b, art| {
        b.push_bind(art.id)
            .push_bind(art.name)
            .push_bind(art.status)
            .push_bind(art.parsed_deployed_on)
            .push_bind(art.package_id)
            .push_bind(tenant_id);
    });

    qb.push(
        " ON CONFLICT (id) DO UPDATE SET \
         name = EXCLUDED.name, \
         status = EXCLUDED.status, \
         deployed_on = EXCLUDED.deployed_on, \
         package_id = COALESCE(EXCLUDED.package_id, runtime_artifacts.package_id), \
         tenant_id = COALESCE(EXCLUDED.tenant_id, runtime_artifacts.tenant_id)",
    );

    qb.build().execute(pool).await?;
    Ok(())
}

pub async fn insert_artifact_error(
    pool: &sqlx::PgPool,
    tenant_id: &str,
    error: ArtifactError,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO artifact_errors (artifact_id, error_message, error_time, tenant_id)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (artifact_id) DO UPDATE SET \
         error_message = EXCLUDED.error_message, \
         error_time = EXCLUDED.error_time, \
         tenant_id = EXCLUDED.tenant_id",
    )
    .bind(error.artifact_id)
    .bind(error.error_message)
    .bind(error.error_time)
    .bind(tenant_id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn bulk_update_artifact_package(
    pool: &sqlx::PgPool,
    mappings: &[(String, String)],
) -> Result<()> {
    for chunk in mappings.chunks(MAPPING_UPDATE_BATCH_SIZE) {
        if chunk.is_empty() {
            continue;
        }

        let pkg_ids: Vec<&str> = chunk.iter().map(|(p, _)| p.as_str()).collect();
        let art_ids: Vec<&str> = chunk.iter().map(|(_, a)| a.as_str()).collect();

        sqlx::query(
            "UPDATE runtime_artifacts SET package_id = updates.pkg_id
             FROM UNNEST($1::text[], $2::text[]) AS updates(pkg_id, art_id)
             WHERE runtime_artifacts.id = updates.art_id",
        )
        .bind(&pkg_ids)
        .bind(&art_ids)
        .execute(pool)
        .await?;
    }

    Ok(())
}

pub async fn insert_configurations(
    pool: &sqlx::PgPool,
    tenant_id: &str,
    artifact_id: &str,
    configs: Vec<crate::models::ArtifactConfiguration>,
) -> Result<()> {
    if configs.is_empty() {
        return Ok(());
    }

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
        "INSERT INTO artifact_configurations (artifact_id, parameter_key, parameter_value, data_type, tenant_id) ",
    );

    qb.push_values(configs, |mut b, cfg| {
        b.push_bind(artifact_id)
            .push_bind(cfg.parameter_key)
            .push_bind(cfg.parameter_value)
            .push_bind(cfg.data_type)
            .push_bind(tenant_id);
    });

    qb.push(
        " ON CONFLICT (artifact_id, parameter_key) DO UPDATE SET \
             parameter_value = EXCLUDED.parameter_value, \
             data_type = EXCLUDED.data_type, \
             tenant_id = EXCLUDED.tenant_id",
    );

    qb.build().execute(pool).await?;
    Ok(())
}

pub async fn get_latest_log_date(
    pool: &sqlx::PgPool,
    tenant_id: &str,
) -> Result<Option<chrono::NaiveDateTime>> {
    let row: Option<(Option<chrono::NaiveDateTime>,)> =
        sqlx::query_as("SELECT MAX(parsed_date) FROM sap_monitoring_logs WHERE tenant_id = $1")
            .bind(tenant_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|(d,)| d))
}

// ─── Types des vues (utilisés par l'UI) ─────────────────────────────────────

#[derive(sqlx::FromRow, Clone)]
pub struct LogView {
    pub status: Option<String>,
    pub parsed_date: Option<chrono::NaiveDateTime>,
    pub error_message: Option<String>,
    pub message_guid: Option<String>,
    pub integration_flow_name: Option<String>,
    pub tenant_id: Option<String>,
}

#[derive(sqlx::FromRow, Clone)]
pub struct ArtifactView {
    pub id: Option<String>,
    pub package_id: Option<String>,
    pub name: Option<String>,
    pub status: Option<String>,
    pub deployed_on: Option<chrono::NaiveDateTime>,
    pub tenant_id: Option<String>,
}

#[derive(sqlx::FromRow, Clone)]
pub struct PackageView {
    pub id: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    pub vendor: Option<String>,
    pub creation_date: Option<chrono::NaiveDateTime>,
    pub tags: Option<String>,
    pub tenant_id: Option<String>,
}

#[derive(sqlx::FromRow, Clone)]
pub struct ErrorView {
    pub artifact_id: String,
    pub error_message: Option<String>,
    pub error_time: Option<chrono::NaiveDateTime>,
    pub tenant_id: Option<String>,
}

// ─── Pending Alerts (Webhook debouncing) ─────────────────────────────────────

pub async fn insert_pending_alert(
    pool: &sqlx::PgPool,
    tenant_id: &str,
    log_guid: &str,
    flow_name: &str,
    error_type: &str,
    error_snippet: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO pending_alerts (log_guid, flow_name, error_type, error_snippet, tenant_id, detected_at)
         VALUES ($1, $2, $3, $4, $5, NOW())
         ON CONFLICT (log_guid) DO NOTHING",
    )
    .bind(log_guid)
    .bind(flow_name)
    .bind(error_type)
    .bind(error_snippet)
    .bind(tenant_id)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct PendingAlert {
    pub id: i64,
    pub log_guid: String,
    pub flow_name: String,
    pub error_type: String,
    pub error_snippet: String,
    pub tenant_id: String,
    pub detected_at: chrono::NaiveDateTime,
}

pub async fn fetch_pending_alerts(pool: &sqlx::PgPool) -> Result<Vec<PendingAlert>> {
    let rows = sqlx::query_as::<_, PendingAlert>(
        "SELECT id, log_guid, flow_name, error_type, error_snippet, tenant_id, detected_at
         FROM pending_alerts
         ORDER BY detected_at ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn delete_pending_alerts(pool: &sqlx::PgPool, ids: &[i64]) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    sqlx::query("DELETE FROM pending_alerts WHERE id = ANY($1)")
        .bind(ids)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn ensure_pending_alerts_table(pool: &sqlx::PgPool) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS pending_alerts (
            id            BIGSERIAL PRIMARY KEY,
            log_guid      TEXT NOT NULL,
            flow_name     TEXT NOT NULL DEFAULT '',
            error_type    TEXT NOT NULL DEFAULT 'exec',
            error_snippet TEXT NOT NULL DEFAULT '',
            tenant_id     VARCHAR(100) NOT NULL DEFAULT 'cerealog',
            detected_at   TIMESTAMP NOT NULL DEFAULT NOW(),
            CONSTRAINT    pending_alerts_guid_uq UNIQUE (log_guid)
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

// ─── Smart Alerts (intelligent alerting) ────────────────────────────────────

pub async fn ensure_smart_alerts_table(pool: &sqlx::PgPool) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS smart_alerts (
            id                BIGSERIAL PRIMARY KEY,
            tenant_id         VARCHAR(100) NOT NULL,
            flow_name         TEXT NOT NULL,
            alert_type        TEXT NOT NULL,
            status            TEXT NOT NULL DEFAULT 'PENDING',
            last_triggered_at TIMESTAMP NOT NULL DEFAULT NOW(),
            extra             JSONB,
            CONSTRAINT smart_alerts_unique UNIQUE (tenant_id, flow_name, alert_type)
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct SmartAlert {
    pub id: i64,
    pub tenant_id: String,
    pub flow_name: String,
    pub alert_type: String,
    pub status: String,
    pub last_triggered_at: chrono::NaiveDateTime,
    pub extra: Option<serde_json::Value>,
}

pub async fn upsert_smart_alert(
    pool: &sqlx::PgPool,
    tenant_id: &str,
    flow_name: &str,
    alert_type: &str,
    extra: Option<serde_json::Value>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO smart_alerts (tenant_id, flow_name, alert_type, status, last_triggered_at, extra)
         VALUES ($1, $2, $3, 'PENDING', NOW(), $4)
         ON CONFLICT (tenant_id, flow_name, alert_type) DO UPDATE SET
           last_triggered_at = EXCLUDED.last_triggered_at,
           status            = 'PENDING',
           extra             = EXCLUDED.extra",
    )
    .bind(tenant_id)
    .bind(flow_name)
    .bind(alert_type)
    .bind(extra)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn fetch_pending_smart_alerts(pool: &sqlx::PgPool) -> Result<Vec<SmartAlert>> {
    sqlx::query_as::<_, SmartAlert>(
        "SELECT id, tenant_id, flow_name, alert_type, status, last_triggered_at, extra
         FROM smart_alerts
         WHERE status = 'PENDING'
         ORDER BY last_triggered_at ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(Into::into)
}

pub async fn mark_smart_alerts_sent(pool: &sqlx::PgPool, ids: &[i64]) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    sqlx::query("UPDATE smart_alerts SET status = 'SENT' WHERE id = ANY($1)")
        .bind(ids)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn fetch_recent_failed_logs_for_alerts(
    pool: &sqlx::PgPool,
    tenant_id: &str,
    limit: i64,
) -> Result<Vec<LogView>> {
    let rows = sqlx::query_as::<_, LogView>(
        "SELECT status, parsed_date, error_message, message_guid, integration_flow_name, tenant_id
         FROM sap_monitoring_logs
         WHERE tenant_id = $1
           AND status = 'FAILED'
           AND message_guid IS NOT NULL
           AND integration_flow_name IS NOT NULL
         ORDER BY parsed_date DESC NULLS LAST
         LIMIT $2",
    )
    .bind(tenant_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
