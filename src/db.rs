use crate::models::{ArtifactError, IntegrationPackage, LogEntry, RuntimeArtifact};
use anyhow::Result;
use sqlx::{Postgres, QueryBuilder};

// ─── Insertions ──────────────────────────────────────────────────────────────

pub async fn insert_logs(pool: &sqlx::PgPool, logs: Vec<LogEntry>) -> Result<()> {
    if logs.is_empty() {
        return Ok(());
    }

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
        "INSERT INTO sap_monitoring_logs \
         (message_guid, status, parsed_date, error_message, integration_flow_name) ",
    );

    qb.push_values(logs, |mut b, log| {
        b.push_bind(log.message_guid)
            .push_bind(log.status)
            .push_bind(log.parsed_date)
            .push_bind(log.error_message)
            .push_bind(log.integration_flow_name);
    });

    qb.push(
        " ON CONFLICT (message_guid) DO UPDATE SET \
         status = EXCLUDED.status, \
         parsed_date = EXCLUDED.parsed_date, \
         error_message = EXCLUDED.error_message, \
         integration_flow_name = EXCLUDED.integration_flow_name",
    );

    qb.build().execute(pool).await?;
    Ok(())
}

pub async fn insert_packages(pool: &sqlx::PgPool, packages: Vec<IntegrationPackage>) -> Result<()> {
    if packages.is_empty() {
        return Ok(());
    }

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
        "INSERT INTO integration_packages (id, name, version, vendor, creation_date, tags) ",
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
            .push_bind(final_tags);
    });

    qb.push(
        " ON CONFLICT (id) DO UPDATE SET \
             name = EXCLUDED.name, \
             version = EXCLUDED.version, \
             vendor = EXCLUDED.vendor, \
             creation_date = EXCLUDED.creation_date, \
             tags = EXCLUDED.tags",
    );

    qb.build().execute(pool).await?;
    Ok(())
}

pub async fn insert_artifacts(pool: &sqlx::PgPool, artifacts: Vec<RuntimeArtifact>) -> Result<()> {
    if artifacts.is_empty() {
        return Ok(());
    }

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
        "INSERT INTO runtime_artifacts (id, name, status, deployed_on, package_id) ",
    );

    qb.push_values(artifacts, |mut b, art| {
        b.push_bind(art.id)
            .push_bind(art.name)
            .push_bind(art.status)
            .push_bind(art.parsed_deployed_on)
            .push_bind(art.package_id);
    });

    qb.push(
        " ON CONFLICT (id) DO UPDATE SET \
         name = EXCLUDED.name, \
         status = EXCLUDED.status, \
         deployed_on = EXCLUDED.deployed_on, \
         package_id = EXCLUDED.package_id",
    );

    qb.build().execute(pool).await?;
    Ok(())
}

pub async fn insert_artifact_error(pool: &sqlx::PgPool, error: ArtifactError) -> Result<()> {
    sqlx::query(
        "INSERT INTO artifact_errors (artifact_id, error_message, error_time)
         VALUES ($1, $2, $3)
         ON CONFLICT (artifact_id) DO UPDATE SET \
         error_message = EXCLUDED.error_message, \
         error_time = EXCLUDED.error_time",
    )
    .bind(error.artifact_id)
    .bind(error.error_message)
    .bind(error.error_time)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn bulk_update_artifact_package(
    pool: &sqlx::PgPool,
    mappings: &[(String, String)],
) -> Result<()> {
    if mappings.is_empty() {
        return Ok(());
    }

    let pkg_ids: Vec<&str> = mappings.iter().map(|(p, _)| p.as_str()).collect();
    let art_ids: Vec<&str> = mappings.iter().map(|(_, a)| a.as_str()).collect();

    sqlx::query(
        "UPDATE runtime_artifacts SET package_id = updates.pkg_id
         FROM UNNEST($1::text[], $2::text[]) AS updates(pkg_id, art_id)
         WHERE runtime_artifacts.id = updates.art_id",
    )
    .bind(&pkg_ids)
    .bind(&art_ids)
    .execute(pool)
    .await?;

    Ok(())
}

// ─── Types des vues (utilisés par l'UI) ─────────────────────────────────────

#[derive(sqlx::FromRow, Clone)]
pub struct ArtifactView {
    pub id: Option<String>,
    pub package_id: Option<String>,
    pub name: Option<String>,
    pub status: Option<String>,
    pub deployed_on: Option<chrono::NaiveDateTime>,
}

#[derive(sqlx::FromRow, Clone)]
pub struct PackageView {
    pub id: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    pub vendor: Option<String>,
    pub creation_date: Option<chrono::NaiveDateTime>,
    pub tags: Option<String>,
}

#[derive(sqlx::FromRow, Clone)]
pub struct ErrorView {
    pub artifact_id: String,
    pub error_message: Option<String>,
    pub error_time: Option<chrono::NaiveDateTime>,
}

#[derive(sqlx::FromRow, Clone)]
pub struct LogView {
    pub status: Option<String>,
    pub parsed_date: Option<chrono::NaiveDateTime>,
    pub error_message: Option<String>,
    pub message_guid: Option<String>,
    pub integration_flow_name: Option<String>,
}

pub async fn insert_configurations(
    pool: &sqlx::PgPool,
    artifact_id: &str,
    configs: Vec<crate::models::ArtifactConfiguration>,
) -> Result<()> {
    if configs.is_empty() {
        return Ok(());
    }

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
        "INSERT INTO artifact_configurations (artifact_id, parameter_key, parameter_value, data_type) ",
    );

    qb.push_values(configs, |mut b, cfg| {
        b.push_bind(artifact_id)
            .push_bind(cfg.parameter_key)
            .push_bind(cfg.parameter_value)
            .push_bind(cfg.data_type);
    });

    qb.push(" ON CONFLICT (artifact_id, parameter_key) DO UPDATE SET parameter_value = EXCLUDED.parameter_value, data_type = EXCLUDED.data_type");

    qb.build().execute(pool).await?;
    Ok(())
}

/// Retourne le timestamp du log le plus récent en base, pour le fetch incrémental.
pub async fn get_latest_log_date(pool: &sqlx::PgPool) -> Result<Option<chrono::NaiveDateTime>> {
    let row: Option<(Option<chrono::NaiveDateTime>,)> =
        sqlx::query_as("SELECT MAX(parsed_date) FROM sap_monitoring_logs")
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|(d,)| d))
}

// ─── Pending Alerts (Webhook debouncing) ─────────────────────────────────────

/// Insère une alerte en attente. `error_type` = "exec" ou "deploy".
pub async fn insert_pending_alert(
    pool: &sqlx::PgPool,
    log_guid: &str,
    flow_name: &str,
    error_type: &str,
    error_snippet: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO pending_alerts (log_guid, flow_name, error_type, error_snippet, detected_at)
         VALUES ($1, $2, $3, $4, NOW())
         ON CONFLICT (log_guid) DO NOTHING",
    )
    .bind(log_guid)
    .bind(flow_name)
    .bind(error_type)
    .bind(error_snippet)
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
    pub detected_at: chrono::NaiveDateTime,
}

/// Lit toutes les alertes en attente, regroupées.
pub async fn fetch_pending_alerts(pool: &sqlx::PgPool) -> Result<Vec<PendingAlert>> {
    let rows = sqlx::query_as::<_, PendingAlert>(
        "SELECT id, log_guid, flow_name, error_type, error_snippet, detected_at
         FROM pending_alerts
         ORDER BY detected_at ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Supprime les alertes envoyées par leurs IDs.
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

/// DDL pour créer la table pending_alerts si elle n'existe pas.
pub async fn ensure_pending_alerts_table(pool: &sqlx::PgPool) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS pending_alerts (
            id           BIGSERIAL PRIMARY KEY,
            log_guid     TEXT NOT NULL,
            flow_name    TEXT NOT NULL DEFAULT '',
            error_type   TEXT NOT NULL DEFAULT 'exec',
            error_snippet TEXT NOT NULL DEFAULT '',
            detected_at  TIMESTAMP NOT NULL DEFAULT NOW(),
            CONSTRAINT pending_alerts_guid_uq UNIQUE (log_guid)
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}
