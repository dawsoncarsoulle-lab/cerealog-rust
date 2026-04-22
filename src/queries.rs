/// Ce module centralise toutes les requêtes de lecture vers la base de données.
/// Les fonctions d'écriture restent dans db.rs.
use crate::db::{ArtifactView, ErrorView, LogView, PackageView};
use crate::ui::{App, Stats};
use anyhow::Result;
use std::collections::HashMap;

/// Rafraîchit toutes les données de l'App depuis la BDD.
/// Les requêtes indépendantes sont exécutées en parallèle via tokio::join!
pub async fn refresh_all(app: &mut App, pool: &sqlx::PgPool) -> Result<()> {
    let (logs_res, artifacts_res, packages_res, errors_res, configs_res) = tokio::join!(
        fetch_logs(pool, app.logs_limit),
        fetch_artifacts(pool),
        fetch_packages(pool),
        fetch_errors(pool),
        fetch_all_configs(pool),
    );

    app.logs = logs_res?;
    app.artifacts = artifacts_res?;
    app.packages = packages_res?;
    app.errors = errors_res?;
    app.configs = configs_res?;

    // ... (le reste de refresh_all ne change pas)

    // ── Groupe 2 : stats & graphiques (parallèle) ────────────────────────────
    let (stats_res, hourly_res, daily_res, activity_res, top_errors_res, log_st_res, art_st_res) = tokio::join!(
        fetch_stats(pool),
        fetch_hourly_errors(pool),
        fetch_daily_errors(pool),
        fetch_activity(pool),
        fetch_top_errors(pool),
        fetch_log_statuses(pool),
        fetch_artifact_statuses(pool),
    );

    app.stats = stats_res?;
    app.error_sparkline = hourly_res?;
    app.error_barchart = daily_res?;
    app.activity_sparkline = activity_res?;
    app.top_errors_barchart = top_errors_res?;

    // Fusion des statuts logs + artifacts
    let mut all_statuses: HashMap<String, u64> = HashMap::new();
    for (s, c) in log_st_res? {
        *all_statuses.entry(s).or_insert(0) += c as u64;
    }
    for (s, c) in art_st_res? {
        *all_statuses.entry(s).or_insert(0) += c as u64;
    }
    app.status_counts = all_statuses.into_iter().collect();

    app.last_refresh = std::time::Instant::now();
    app.apply_filters();

    Ok(())
}

// ─── Requêtes individuelles ──────────────────────────────────────────────────

async fn fetch_logs(pool: &sqlx::PgPool, limit: u32) -> Result<Vec<LogView>> {
    let rows = sqlx::query_as(
        "SELECT status, parsed_date, error_message, message_guid, integration_flow_name
         FROM sap_monitoring_logs
         ORDER BY parsed_date DESC NULLS LAST
         LIMIT $1",
    )
    .bind(limit as i64)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

async fn fetch_artifacts(pool: &sqlx::PgPool) -> Result<Vec<ArtifactView>> {
    let rows = sqlx::query_as(
        "SELECT id, package_id, name, status, deployed_on
         FROM runtime_artifacts
         ORDER BY name ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

async fn fetch_packages(pool: &sqlx::PgPool) -> Result<Vec<PackageView>> {
    let rows = sqlx::query_as(
        "SELECT id, name, version, vendor, creation_date, tags
         FROM integration_packages
         ORDER BY name ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

async fn fetch_errors(pool: &sqlx::PgPool) -> Result<Vec<ErrorView>> {
    let rows = sqlx::query_as(
        "SELECT artifact_id, error_message, error_time
         FROM artifact_errors
         ORDER BY error_time DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

async fn fetch_stats(pool: &sqlx::PgPool) -> Result<Stats> {
    // Une seule requête agrégée au lieu de 4 COUNT(*) séparés
    let row: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT
            (SELECT COUNT(*) FROM sap_monitoring_logs),
            (SELECT COUNT(*) FROM sap_monitoring_logs WHERE status = 'FAILED'),
            (SELECT COUNT(*) FROM integration_packages),
            (SELECT COUNT(*) FROM runtime_artifacts)",
    )
    .fetch_one(pool)
    .await?;

    Ok(Stats {
        total_logs: row.0,
        failed_logs: row.1,
        total_packages: row.2,
        total_artifacts: row.3,
    })
}

async fn fetch_hourly_errors(pool: &sqlx::PgPool) -> Result<Vec<u64>> {
    let rows: Vec<(i64,)> = sqlx::query_as(
        "SELECT COUNT(*) FROM sap_monitoring_logs
         WHERE status = 'FAILED'
           AND parsed_date > NOW() - INTERVAL '24 hours'
         GROUP BY date_trunc('hour', parsed_date)
         ORDER BY date_trunc('hour', parsed_date)",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(|(c,)| *c as u64).collect())
}

async fn fetch_daily_errors(pool: &sqlx::PgPool) -> Result<Vec<(String, u64)>> {
    let rows: Vec<(chrono::NaiveDate, i64)> = sqlx::query_as(
        "SELECT parsed_date::date, COUNT(*)
         FROM sap_monitoring_logs
         WHERE status = 'FAILED' AND parsed_date > NOW() - INTERVAL '7 days'
         GROUP BY parsed_date::date ORDER BY 1",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|(d, c)| (d.format("%a").to_string(), *c as u64))
        .collect())
}

async fn fetch_activity(pool: &sqlx::PgPool) -> Result<Vec<u64>> {
    let rows: Vec<(i64,)> = sqlx::query_as(
        "SELECT COUNT(*) FROM sap_monitoring_logs
         WHERE parsed_date > NOW() - INTERVAL '12 hours'
         GROUP BY date_trunc('hour', parsed_date)
         ORDER BY date_trunc('hour', parsed_date)",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(|(c,)| *c as u64).collect())
}

async fn fetch_top_errors(pool: &sqlx::PgPool) -> Result<Vec<(String, u64)>> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT artifact_id, COUNT(*) as cnt
         FROM artifact_errors
         GROUP BY artifact_id
         ORDER BY cnt DESC
         LIMIT 5",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(id, c)| (id, c as u64)).collect())
}

async fn fetch_log_statuses(pool: &sqlx::PgPool) -> Result<Vec<(String, i64)>> {
    let rows = sqlx::query_as(
        "SELECT status, COUNT(*) FROM sap_monitoring_logs WHERE status IS NOT NULL GROUP BY status",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

async fn fetch_artifact_statuses(pool: &sqlx::PgPool) -> Result<Vec<(String, i64)>> {
    let rows = sqlx::query_as(
        "SELECT status, COUNT(*) FROM runtime_artifacts WHERE status IS NOT NULL GROUP BY status",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

async fn fetch_all_configs(pool: &sqlx::PgPool) -> Result<HashMap<String, Vec<(String, String)>>> {
    let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT artifact_id, parameter_key, parameter_value FROM artifact_configurations",
    )
    .fetch_all(pool)
    .await?;

    let mut map: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for (id, key, val) in rows {
        map.entry(id)
            .or_default()
            .push((key, val.unwrap_or_else(|| "—".to_string())));
    }
    Ok(map)
}
