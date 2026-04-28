use crate::db::{ArtifactView, ErrorView, LogView, PackageView};
use crate::models::{RefreshData, Stats};
use anyhow::Result;
use sqlx::Row;
use std::collections::HashMap;

/// Construit le payload complet de rafraîchissement.
pub async fn build_refresh_data(pool: &sqlx::PgPool, logs_limit: u32) -> Result<RefreshData> {
    let (
        logs_res,
        exec_errors_res,
        active_exec_errors_res,
        artifacts_res,
        packages_res,
        deploy_errors_res,
        configs_res,
        stats_res,
        hourly_res,
        daily_res,
        activity_res,
        top_errors_res,
        log_st_res,
        art_st_res,
    ) = tokio::join!(
        fetch_logs(pool, logs_limit),
        fetch_exec_errors(pool),
        fetch_active_exec_errors(pool),
        fetch_artifacts(pool),
        fetch_packages(pool),
        fetch_deploy_errors(pool),
        fetch_all_configs(pool),
        fetch_stats(pool),
        fetch_hourly_errors(pool),
        fetch_daily_errors(pool),
        fetch_activity(pool),
        fetch_top_errors(pool),
        fetch_log_statuses(pool),
        fetch_artifact_statuses(pool),
    );

    let mut all_statuses: HashMap<String, u64> = HashMap::new();
    for (s, c) in log_st_res? {
        *all_statuses.entry(s).or_insert(0) += c as u64;
    }
    for (s, c) in art_st_res? {
        *all_statuses.entry(s).or_insert(0) += c as u64;
    }

    Ok(RefreshData {
        logs: logs_res?,
        exec_errors: exec_errors_res?,
        active_exec_errors: active_exec_errors_res?,
        artifacts: artifacts_res?,
        packages: packages_res?,
        deploy_errors: deploy_errors_res?,
        configs: configs_res?,
        stats: stats_res?,
        error_sparkline: hourly_res?,
        error_barchart: daily_res?,
        activity_sparkline: activity_res?,
        top_errors_barchart: top_errors_res?,
        status_counts: all_statuses.into_iter().collect(),
    })
}

// ─── Requêtes individuelles ───────────────────────────────────────────────────

async fn fetch_logs(pool: &sqlx::PgPool, limit: u32) -> Result<Vec<LogView>> {
    let rows = sqlx::query_as(
        "SELECT status, parsed_date, error_message, message_guid, integration_flow_name, tenant_id
         FROM sap_monitoring_logs
         ORDER BY parsed_date DESC NULLS LAST
         LIMIT $1",
    )
    .bind(limit as i64)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// MPL FAILED uniquement
pub async fn fetch_exec_errors(pool: &sqlx::PgPool) -> Result<Vec<LogView>> {
    let rows = sqlx::query_as(
        "SELECT status, parsed_date, error_message, message_guid, integration_flow_name, tenant_id
         FROM sap_monitoring_logs
         WHERE status = 'FAILED'
         ORDER BY parsed_date DESC NULLS LAST
         LIMIT 200",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

async fn fetch_artifacts(pool: &sqlx::PgPool) -> Result<Vec<ArtifactView>> {
    let rows = sqlx::query_as(
        "SELECT id, package_id, name, status, deployed_on, tenant_id
         FROM runtime_artifacts
         ORDER BY name ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

async fn fetch_packages(pool: &sqlx::PgPool) -> Result<Vec<PackageView>> {
    let rows = sqlx::query_as(
        "SELECT id, name, version, vendor, creation_date, tags, tenant_id
         FROM integration_packages
         ORDER BY name ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Erreurs de déploiement (artifact_errors).
async fn fetch_deploy_errors(pool: &sqlx::PgPool) -> Result<Vec<ErrorView>> {
    let rows = sqlx::query_as(
        "SELECT artifact_id, error_message, error_time, tenant_id
         FROM artifact_errors
         ORDER BY error_time DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

async fn fetch_stats(pool: &sqlx::PgPool) -> Result<Stats> {
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
        "SELECT status, COUNT(*) FROM sap_monitoring_logs \
         WHERE status IS NOT NULL GROUP BY status",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

async fn fetch_artifact_statuses(pool: &sqlx::PgPool) -> Result<Vec<(String, i64)>> {
    let rows = sqlx::query_as(
        "SELECT status, COUNT(*) FROM runtime_artifacts \
         WHERE status IS NOT NULL GROUP BY status",
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

pub async fn fetch_active_exec_errors(pool: &sqlx::PgPool) -> Result<Vec<LogView>> {
    let rows = sqlx::query_as(
        "SELECT status, parsed_date, error_message, message_guid, integration_flow_name, tenant_id
         FROM sap_monitoring_logs l
         WHERE status = 'FAILED'
           AND integration_flow_name IS NOT NULL
           AND parsed_date = (
               SELECT MAX(l2.parsed_date)
               FROM sap_monitoring_logs l2
               WHERE l2.integration_flow_name = l.integration_flow_name
                 AND l2.tenant_id = l.tenant_id
           )
         ORDER BY parsed_date DESC NULLS LAST
         LIMIT 200",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

// ─── Détection d'alertes intelligentes ────────────────────────────────────────

/// Détecte les pics (spikes) d'erreurs pour chaque flux et locataire. Un pic est
/// caractérisé par un nombre d'échecs dans les 10 dernières minutes au moins
/// deux fois supérieur à la moyenne des 10 minutes sur l'heure précédente.
/// Retourne un vecteur de `(tenant_id, flow_name, recent_count, avg_count)`.
pub async fn detect_spike(pool: &sqlx::PgPool) -> Result<Vec<(String, String, i64, f64)>> {
    let rows = sqlx::query(
        "SELECT c.tenant_id, c.integration_flow_name, c.recent_count, c.avg_count
         FROM (
            SELECT r.tenant_id,
                   r.integration_flow_name,
                   COUNT(*) AS recent_count,
                   (COALESCE((
                     SELECT COUNT(*)
                     FROM sap_monitoring_logs s2
                     WHERE s2.tenant_id = r.tenant_id
                       AND s2.integration_flow_name = r.integration_flow_name
                       AND s2.status = 'FAILED'
                       AND s2.parsed_date <= NOW() - INTERVAL '10 minutes'
                       AND s2.parsed_date > NOW() - INTERVAL '70 minutes'
                   ),0)::double precision / 6.0::double precision) AS avg_count
            FROM sap_monitoring_logs r
            WHERE r.status = 'FAILED'
              AND r.parsed_date > NOW() - INTERVAL '10 minutes'
            GROUP BY r.tenant_id, r.integration_flow_name
         ) c
         WHERE c.recent_count >= 5
           AND c.recent_count > c.avg_count * 2
           AND NOT EXISTS (
               SELECT 1
               FROM smart_alerts sa
               WHERE sa.tenant_id = c.tenant_id
                 AND sa.flow_name = c.integration_flow_name
                 AND sa.alert_type = 'SPIKE'
                 AND sa.last_triggered_at > NOW() - INTERVAL '30 minutes'
           )",
    )
    .fetch_all(pool)
    .await?;

    let mut results = Vec::new();
    for row in rows {
        let tenant_id: String = row.try_get(0)?;
        let flow_name: String = row.try_get(1)?;
        let recent_count: i64 = row.try_get(2)?;
        let avg_count: f64 = row.try_get(3)?;
        results.push((tenant_id, flow_name, recent_count, avg_count));
    }
    Ok(results)
}

/// Détecte les régressions d'un flux : lorsque la dernière exécution est en
/// échec (`FAILED`) et que l'exécution précédente n'était pas en échec. Les
/// régressions déjà traitées au cours des 30 dernières minutes sont ignorées.
pub async fn detect_regression(pool: &sqlx::PgPool) -> Result<Vec<(String, String)>> {
    let rows = sqlx::query(
        "WITH ranked AS (
            SELECT tenant_id, integration_flow_name, status,
                   ROW_NUMBER() OVER (PARTITION BY tenant_id, integration_flow_name ORDER BY parsed_date DESC) AS rn
            FROM sap_monitoring_logs
        ), last_two AS (
            SELECT tenant_id, integration_flow_name,
                   MAX(CASE WHEN rn = 1 THEN status END) AS last_status,
                   MAX(CASE WHEN rn = 2 THEN status END) AS prev_status
            FROM ranked
            WHERE rn <= 2
            GROUP BY tenant_id, integration_flow_name
        )
        SELECT tenant_id, integration_flow_name
        FROM last_two
        WHERE last_status = 'FAILED'
          AND (prev_status IS NULL OR prev_status != 'FAILED')
          AND NOT EXISTS (
              SELECT 1
              FROM smart_alerts sa
              WHERE sa.tenant_id = last_two.tenant_id
                AND sa.flow_name = last_two.integration_flow_name
                AND sa.alert_type = 'REGRESSION'
                AND sa.last_triggered_at > NOW() - INTERVAL '30 minutes'
          )",
    )
    .fetch_all(pool)
    .await?;

    let mut results = Vec::new();
    for row in rows {
        let tenant_id: String = row.try_get(0)?;
        let flow_name: String = row.try_get(1)?;
        results.push((tenant_id, flow_name));
    }
    Ok(results)
}

/// Détecte les pannes persistantes d'un flux : lorsqu'aucune exécution
/// réussie (statut différent de `FAILED`) n'a eu lieu depuis plus de 30
/// minutes et que la dernière exécution est un échec. Retourne un vecteur
/// `(tenant_id, flow_name, durée_en_minutes)` pour chaque flux en panne. Les
/// pannes déjà signalées dans les 30 dernières minutes sont ignorées.
pub async fn detect_persistent_failure(pool: &sqlx::PgPool) -> Result<Vec<(String, String, i64)>> {
    let rows = sqlx::query(
        "WITH last_dates AS (
            SELECT tenant_id, integration_flow_name,
                   MAX(CASE WHEN status = 'FAILED' THEN parsed_date END) AS last_failed,
                   MAX(CASE WHEN status != 'FAILED' THEN parsed_date END) AS last_success
            FROM sap_monitoring_logs
            GROUP BY tenant_id, integration_flow_name
        )
        SELECT tenant_id, integration_flow_name,
               (EXTRACT(EPOCH FROM (NOW() - COALESCE(last_success, NOW())))::double precision / 60.0::double precision) AS fail_duration
        FROM last_dates
        WHERE (last_success IS NULL OR last_success < NOW() - INTERVAL '30 minutes')
          AND last_failed >= COALESCE(last_success, last_failed)
          AND NOT EXISTS (
              SELECT 1 FROM smart_alerts sa
              WHERE sa.tenant_id = last_dates.tenant_id
                AND sa.flow_name = last_dates.integration_flow_name
                AND sa.alert_type = 'PERSISTENT'
                AND sa.last_triggered_at > NOW() - INTERVAL '30 minutes'
          )",
    )
    .fetch_all(pool)
    .await?;

    let mut results = Vec::new();
    for row in rows {
        let tenant_id: String = row.try_get(0)?;
        let flow_name: String = row.try_get(1)?;
        let duration: f64 = row.try_get(2)?;
        // round the duration to the nearest integer minute
        let mins = duration.round() as i64;
        results.push((tenant_id, flow_name, mins));
    }
    Ok(results)
}
