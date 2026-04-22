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

    refresh_views(app, pool).await?;
    Ok(())
}

async fn refresh_views(app: &mut App, pool: &sqlx::PgPool) -> anyhow::Result<()> {
    app.logs = sqlx::query_as(
        "SELECT status, parsed_date, error_message, message_guid
         FROM sap_monitoring_logs
         ORDER BY parsed_date DESC NULLS LAST
         LIMIT $1",
    )
    .bind(app.logs_limit as i64)
    .fetch_all(pool)
    .await?;

    // NOUVEAU : On récupère l'ID ici !
    app.artifacts = sqlx::query_as(
        "SELECT id, name, status, deployed_on FROM runtime_artifacts ORDER BY name ASC",
    )
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

    app.last_refresh = Instant::now();
    app.apply_filters();

    let activity: Vec<(i64,)> = sqlx::query_as(
        "SELECT COUNT(*) FROM sap_monitoring_logs
         WHERE parsed_date > NOW() - INTERVAL '12 hours'
         GROUP BY date_trunc('hour', parsed_date)
         ORDER BY date_trunc('hour', parsed_date)",
    )
    .fetch_all(pool)
    .await?;
    app.activity_sparkline = activity.iter().map(|(c,)| *c as u64).collect();

    let top_errors: Vec<(String, i64)> = sqlx::query_as(
        "SELECT artifact_id, COUNT(*) as cnt
         FROM artifact_errors
         GROUP BY artifact_id
         ORDER BY cnt DESC
         LIMIT 5",
    )
    .fetch_all(pool)
    .await?;
    app.top_errors_barchart = top_errors
        .into_iter()
        .map(|(id, c)| (id, c as u64))
        .collect();

    // NOUVEAU : On additionne les statuts des Logs ET des Artifacts !
    let mut all_statuses = std::collections::HashMap::new();

    let log_statuses: Vec<(String, i64)> = sqlx::query_as(
        "SELECT status, COUNT(*) FROM sap_monitoring_logs WHERE status IS NOT NULL GROUP BY status",
    )
    .fetch_all(pool)
    .await?;
    for (s, c) in log_statuses {
        *all_statuses.entry(s).or_insert(0) += c as u64;
    }

    let art_statuses: Vec<(String, i64)> = sqlx::query_as(
        "SELECT status, COUNT(*) FROM runtime_artifacts WHERE status IS NOT NULL GROUP BY status",
    )
    .fetch_all(pool)
    .await?;
    for (s, c) in art_statuses {
        *all_statuses.entry(s).or_insert(0) += c as u64;
    }

    app.status_counts = all_statuses.into_iter().collect();

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();

    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL manquante dans .env");
    let pool = sqlx::PgPool::connect(&db_url).await?;

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

    const AUTO_REFRESH: Duration = Duration::from_secs(300);

    loop {
        ui::run_tui(&mut app)?;

        if app.should_quit {
            break;
        }

        if matches!(app.overlay, OverlayState::Running { .. }) {
            app.overlay = OverlayState::Hidden;
            app.should_quit = false;
            let limit = app.logs_limit;
            let pb = create_spinner("Re-extraction SAP...");
            match load_data(&mut app, &pool, limit, false).await {
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

        if matches!(app.overlay, OverlayState::LoadMore) {
            app.overlay = OverlayState::Hidden;
            app.should_quit = false;
            let limit = app.logs_limit;
            let pb = create_spinner(&format!("Chargement de {} logs...", limit));
            match load_data(&mut app, &pool, limit, false).await {
                Ok(()) => pb.finish_with_message("Chargement terminé !"),
                Err(e) => pb.finish_with_message(format!("Erreur: {}", e)),
            }
        }

        if app.last_refresh.elapsed() >= AUTO_REFRESH {
            let limit = app.logs_limit;
            let _ = load_data(&mut app, &pool, limit, false).await;
        }
    }

    println!("SAP BTP Monitor — Au revoir !");
    Ok(())
}
