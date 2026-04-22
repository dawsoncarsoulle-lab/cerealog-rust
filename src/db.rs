use crate::models::{ArtifactError, IntegrationPackage, LogEntry, RuntimeArtifact};
use anyhow::Result;
use sqlx::{Postgres, QueryBuilder};

pub async fn insert_logs(pool: &sqlx::PgPool, logs: Vec<LogEntry>) -> Result<()> {
    if logs.is_empty() {
        return Ok(());
    }

    let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
        "INSERT INTO sap_monitoring_logs (message_guid, status, parsed_date, error_message) ",
    );

    query_builder.push_values(logs, |mut b, log| {
        b.push_bind(log.message_guid)
            .push_bind(log.status)
            .push_bind(log.parsed_date)
            .push_bind(log.error_message);
    });

    query_builder.push(" ON CONFLICT (message_guid) DO UPDATE SET status = EXCLUDED.status, parsed_date = EXCLUDED.parsed_date, error_message = EXCLUDED.error_message");

    query_builder.build().execute(pool).await?;
    Ok(())
}

pub async fn insert_packages(
    pool: &sqlx::PgPool,
    packages: Vec<IntegrationPackage>,
) -> anyhow::Result<()> {
    if packages.is_empty() {
        return Ok(());
    }

    let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
        "INSERT INTO integration_packages (id, name, version, vendor, creation_date) ",
    );

    query_builder.push_values(packages, |mut b, pkg| {
        b.push_bind(pkg.id)
            .push_bind(pkg.name)
            .push_bind(pkg.version)
            .push_bind(pkg.vendor)
            .push_bind(pkg.parsed_creation_date);
    });

    query_builder.push(" ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, version = EXCLUDED.version, vendor = EXCLUDED.vendor, creation_date = EXCLUDED.creation_date");

    query_builder.build().execute(pool).await?;
    Ok(())
}

pub async fn insert_artifacts(pool: &sqlx::PgPool, artifacts: Vec<RuntimeArtifact>) -> Result<()> {
    if artifacts.is_empty() {
        return Ok(());
    }

    let mut query_builder: QueryBuilder<Postgres> =
        QueryBuilder::new("INSERT INTO runtime_artifacts (id, name, status, deployed_on) ");

    query_builder.push_values(artifacts, |mut b, art| {
        b.push_bind(art.id)
            .push_bind(art.name)
            .push_bind(art.status)
            .push_bind(art.parsed_deployed_on);
    });

    query_builder.push(" ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, status = EXCLUDED.status, deployed_on = EXCLUDED.deployed_on");

    query_builder.build().execute(pool).await?;
    Ok(())
}

pub async fn insert_artifact_error(pool: &sqlx::PgPool, error: ArtifactError) -> Result<()> {
    sqlx::query(
        "INSERT INTO artifact_errors (artifact_id, error_message, error_time)
         VALUES ($1, $2, $3)
         ON CONFLICT (artifact_id) DO UPDATE SET error_message = EXCLUDED.error_message, error_time = EXCLUDED.error_time"
    )
    .bind(error.artifact_id)
    .bind(error.error_message)
    .bind(error.error_time)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn clear_all_tables(pool: &sqlx::PgPool) -> Result<()> {
    sqlx::query("TRUNCATE TABLE artifact_errors, runtime_artifacts, integration_packages, sap_monitoring_logs CASCADE")
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_db_summary(pool: &sqlx::PgPool) -> Result<()> {
    let logs_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sap_monitoring_logs")
        .fetch_one(pool)
        .await?;
    let err_count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM sap_monitoring_logs WHERE status = 'FAILED'")
            .fetch_one(pool)
            .await?;
    let pkg_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM integration_packages")
        .fetch_one(pool)
        .await?;
    let runtime: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM runtime_artifacts")
        .fetch_one(pool)
        .await?;

    println!("\nRÉSUMÉ DE LA BASE DE DONNÉES");
    println!("-------------------------------");
    println!("Total Logs : {}", logs_count.0);
    println!("Packages SAP : {}", pkg_count.0);
    println!("Runtime SAP : {}", runtime.0);
    println!("Erreurs détectées : {}", err_count.0);
    println!("-------------------------------\n");
    Ok(())
}

// ─── VUES POUR L'INTERFACE GRAPHIQUE ────────────────────────────────

#[derive(sqlx::FromRow, Clone)]
pub struct ArtifactView {
    pub id: Option<String>, // <--- NOUVEAU : On récupère l'ID pour faire le lien avec l'erreur !
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
}

// ─── ANCIENNES FONCTIONS D'AFFICHAGE CLI ─────────────────────────────

pub async fn get_view_of_runtime(pool: &sqlx::PgPool) -> anyhow::Result<()> {
    let runtime_view: Vec<ArtifactView> = sqlx::query_as(
        "SELECT id, name, status, deployed_on FROM runtime_artifacts ORDER BY name ASC",
    )
    .fetch_all(pool)
    .await?;

    println!("\n=== LISTE DES ARTEFACTS DÉPLOYÉS ===");
    for art in runtime_view {
        let nom = art.name.unwrap_or_else(|| "Nom inconnu".to_string());
        let statut = art.status.unwrap_or_else(|| "N/A".to_string());
        let icon = if statut == "ERROR" { "🔴" } else { "🟢" };

        println!("{} {} (Statut: {})", icon, nom, statut);
    }
    println!("=======================================\n");

    Ok(())
}

pub async fn get_view_of_package(pool: &sqlx::PgPool) -> anyhow::Result<()> {
    let views: Vec<PackageView> =
        sqlx::query_as("SELECT id, name, version, vendor, creation_date FROM integration_packages ORDER BY name ASC")
            .fetch_all(pool)
            .await?;

    println!("\n=== LISTE DES PACKAGES ===");
    for pkg in views {
        println!(
            "📦 {} (v{}) - ID: {}",
            pkg.name.unwrap_or_default(),
            pkg.version.unwrap_or_else(|| "?.?".to_string()),
            pkg.id.unwrap_or_default()
        );
    }
    println!("=============================\n");
    Ok(())
}

pub async fn get_view_of_error(pool: &sqlx::PgPool) -> anyhow::Result<()> {
    let views: Vec<ErrorView> = sqlx::query_as(
        "SELECT artifact_id, error_message, error_time FROM artifact_errors ORDER BY error_time DESC"
    )
    .fetch_all(pool)
    .await?;

    println!("\n=== ERREURS DE DÉPLOIEMENT ===");
    if views.is_empty() {
        println!("Aucune erreur à signaler !");
    } else {
        for err in views {
            let date_str = err
                .error_time
                .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "Date inconnue".to_string());

            println!(
                "❌ [{}] {} : {}",
                date_str,
                err.artifact_id,
                err.error_message.unwrap_or_default()
            );
        }
    }
    println!("=================================\n");
    Ok(())
}

pub async fn get_view_of_logs(pool: &sqlx::PgPool, limit: i64) -> anyhow::Result<()> {
    let views: Vec<LogView> = sqlx::query_as(
        "SELECT status, parsed_date, error_message, message_guid
         FROM sap_monitoring_logs
         ORDER BY parsed_date DESC NULLS LAST
         LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    println!("\n=== {limit} DERNIERS LOGS D'EXÉCUTION ===");
    for log in views {
        let statut = log.status.unwrap_or_default();
        let icon = match statut.as_str() {
            "COMPLETED" => "✅",
            "FAILED" => "❌",
            _ => "⏳",
        };

        let date_str = log
            .parsed_date
            .map(|d| d.format("%d/%m %H:%M:%S").to_string())
            .unwrap_or_else(|| "??/??".to_string());

        if statut == "FAILED" {
            println!(
                "{} [{}] Statut: {} - Détail: {}",
                icon,
                date_str,
                statut,
                log.error_message.unwrap_or_default()
            );
        } else {
            println!("{} [{}] Statut: {}", icon, date_str, statut);
        }
    }
    println!("=======================================\n");
    Ok(())
}
