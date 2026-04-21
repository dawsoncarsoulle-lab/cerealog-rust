mod api;
mod db;
mod models;
mod ui;

use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use std::time::{Duration, Instant};
use ui::{App, OverlayState};

#[derive(Parser, Debug)]
#[command(author, version, about = "SAP BTP Monitor — Dashboard TUI")]
struct Cli {
    #[arg(short, long, help = "Nombre de logs à récupérer au démarrage")]
    top: Option<u32>,
    #[arg(long, help = "Vérifier les connexions et quitter")]
    health: bool,
}

fn create_spinner(msg: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.enable_steady_tick(Duration::from_millis(100));
    pb.set_style(
        ProgressStyle::with_template("{spinner:.cyan} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    pb.set_message(msg.to_string());
    pb
}

/// Charge toutes les données depuis SAP + BDD et remplit l'App
async fn load_data(
    app: &mut App,
    pool: &sqlx::PgPool,
    top: u32,
    only_errors: bool,
) -> anyhow::Result<()> {
    let client = reqwest::Client::new();
    let token = api::get_sap_token(&client).await?;

    let filter = if only_errors {
        Some("Status eq 'FAILED'")
    } else {
        None
    };

    let (logs_res, packages_res, artifacts_res) = tokio::join!(
        api::fetch_sap_logs(&client, &token, top, filter),
        api::fetch_packages(&client, &token),
        api::fetch_artifacts(&client, &token)
    );

    let logs = logs_res?;
    let packages = packages_res?;
    let artifacts = artifacts_res?;

    db::insert_logs(pool, logs).await?;
    db::insert_packages(pool, packages).await?;
    db::insert_artifacts(pool, artifacts.clone()).await?;

    // Récupération erreurs artifacts
    for art in artifacts {
        if art.status.as_deref() == Some("ERROR") {
            if let Some(id) = art.id {
                if let Ok(Some(err_txt)) = api::fetch_artifact_error(&client, &token, &id).await {
                    let err_detail = models::ArtifactError {
                        artifact_id: id,
                        error_message: err_txt,
                        error_time: chrono::Utc::now().naive_utc(),
                    };
                    let _ = db::insert_artifact_error(pool, err_detail).await;
                }
            }
        }
    }

    // Recharger les vues depuis la BDD
    refresh_views(app, pool).await?;
    Ok(())
}

/// Recharge les données locales depuis PostgreSQL (pas d'appel SAP)
async fn refresh_views(app: &mut App, pool: &sqlx::PgPool) -> anyhow::Result<()> {
    app.logs = sqlx::query_as(
        "SELECT status, parsed_date, error_message, message_guid
         FROM sap_monitoring_logs
         ORDER BY parsed_date DESC NULLS LAST
         LIMIT 500",
    )
    .fetch_all(pool)
    .await?;

    app.artifacts =
        sqlx::query_as("SELECT name, status, deployed_on FROM runtime_artifacts ORDER BY name ASC")
            .fetch_all(pool)
            .await?;

    app.packages =
        sqlx::query_as("SELECT id, name, version, vendor, creation_date FROM integration_packages ORDER BY name ASC")
            .fetch_all(pool)
            .await?;

    app.errors = sqlx::query_as(
        "SELECT artifact_id, error_message, error_time FROM artifact_errors ORDER BY error_time DESC",
    )
    .fetch_all(pool)
    .await?;

    // Stats
    let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sap_monitoring_logs")
        .fetch_one(pool)
        .await?;
    let failed: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM sap_monitoring_logs WHERE status = 'FAILED'")
            .fetch_one(pool)
            .await?;
    let pkgs: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM integration_packages")
        .fetch_one(pool)
        .await?;
    let arts: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM runtime_artifacts")
        .fetch_one(pool)
        .await?;

    app.stats.total_logs = total.0;
    app.stats.failed_logs = failed.0;
    app.stats.total_packages = pkgs.0;
    app.stats.total_artifacts = arts.0;

    // --- GRAPHIQUES (Sparkline 24h & BarChart 7j) ---
    let hourly: Vec<(i64,)> = sqlx::query_as(
        "SELECT COUNT(*) FROM sap_monitoring_logs
         WHERE status = 'FAILED'
           AND parsed_date > NOW() - INTERVAL '24 hours'
         GROUP BY date_trunc('hour', parsed_date)
         ORDER BY date_trunc('hour', parsed_date)",
    )
    .fetch_all(pool)
    .await?;
    app.error_sparkline = hourly.iter().map(|(c,)| *c as u64).collect();

    let daily: Vec<(chrono::NaiveDate, i64)> = sqlx::query_as(
        "SELECT parsed_date::date, COUNT(*)
         FROM sap_monitoring_logs
         WHERE status = 'FAILED' AND parsed_date > NOW() - INTERVAL '7 days'
         GROUP BY parsed_date::date ORDER BY 1",
    )
    .fetch_all(pool)
    .await?;
    app.error_barchart = daily
        .iter()
        .map(|(d, c)| (d.format("%a").to_string(), *c as u64))
        .collect();

    // Mise à jour de la date de fraîcheur
    app.last_refresh = Instant::now();
    app.apply_filters(); // Re-filtrer automatiquement après un refresh

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();

    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL manquante dans .env");
    let pool = sqlx::PgPool::connect(&db_url).await?;

    // Mode health-check
    if cli.health {
        let pb = create_spinner("Vérification des connexions...");
        let _ = api::get_sap_token(&reqwest::Client::new()).await?;
        pb.finish_with_message("✓ SAP OK  ✓ DB OK — Système opérationnel");
        return Ok(());
    }

    let top = cli.top.unwrap_or(200);
    {
        let pb = create_spinner(&format!("Chargement initial ({} logs)...", top));
        let client = reqwest::Client::new();
        let token = api::get_sap_token(&client).await?;
        let filter: Option<&str> = None;
        let (lr, pr, ar) = tokio::join!(
            api::fetch_sap_logs(&client, &token, top, filter),
            api::fetch_packages(&client, &token),
            api::fetch_artifacts(&client, &token)
        );
        if let (Ok(logs), Ok(pkgs), Ok(arts)) = (lr, pr, ar) {
            db::insert_logs(&pool, logs).await?;
            db::insert_packages(&pool, pkgs).await?;
            db::insert_artifacts(&pool, arts.clone()).await?;
            for art in arts {
                if art.status.as_deref() == Some("ERROR") {
                    if let Some(id) = art.id {
                        if let Ok(Some(err_txt)) =
                            api::fetch_artifact_error(&client, &token, &id).await
                        {
                            let _ = db::insert_artifact_error(
                                &pool,
                                models::ArtifactError {
                                    artifact_id: id,
                                    error_message: err_txt,
                                    error_time: chrono::Utc::now().naive_utc(),
                                },
                            )
                            .await;
                        }
                    }
                }
            }
        }
        pb.finish_with_message("Données chargées !");
    }

    let mut app = App::new();
    refresh_views(&mut app, &pool).await?;

    // Boucle principale
    loop {
        ui::run_tui(&mut app)?;

        if app.should_quit {
            break;
        }

        if matches!(app.overlay, OverlayState::Running { .. }) {
            app.overlay = OverlayState::Hidden;
            app.should_quit = false;

            let pb = create_spinner("Re-extraction SAP...");
            match load_data(&mut app, &pool, top, false).await {
                Ok(()) => {
                    pb.finish_with_message("Re-extraction terminée !");
                    app.overlay = OverlayState::Done {
                        message: "Données mises à jour avec succès.".to_string(),
                    };
                }
                Err(e) => {
                    pb.finish_with_message("Erreur lors de l'extraction.");
                    app.overlay = OverlayState::Error {
                        message: format!("{}", e),
                    };
                }
            }
        }
    }

    println!("SAP BTP Monitor — Au revoir !");
    Ok(())
}
