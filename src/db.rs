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

/// Met à jour le package_id de plusieurs artifacts en une seule transaction.
pub async fn bulk_update_artifact_package(
    pool: &sqlx::PgPool,
    mappings: &[(String, String)],
) -> Result<()> {
    if mappings.is_empty() {
        return Ok(());
    }

    let mut tx = pool.begin().await?;
    for (pkg_id, art_id) in mappings {
        sqlx::query("UPDATE runtime_artifacts SET package_id = $1 WHERE id = $2")
            .bind(pkg_id)
            .bind(art_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
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
