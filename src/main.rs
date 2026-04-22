mod api;
mod db;
mod models;
mod queries;
mod ui;
mod utils;

use clap::Parser;
use std::time::Duration;
use ui::{App, AppEvent};

#[derive(Parser, Debug)]
#[command(author, version, about = "SAP BTP Monitor — Dashboard TUI")]
struct Cli {
    #[arg(short, long, help = "Nombre de logs à récupérer au démarrage")]
    top: Option<u32>,
    #[arg(long, help = "Vérifier les connexions et quitter")]
    health: bool,
}

async fn load_data(app: &mut App, pool: &sqlx::PgPool, top: u32) -> anyhow::Result<()> {
    use futures::stream::{self, StreamExt};

    let client = api::build_http_client()?;
    let token = api::get_sap_token(&client).await?;

    let (logs_res, packages_res, artifacts_res) = tokio::join!(
        api::fetch_sap_logs(&client, &token, top, None),
        api::fetch_packages(&client, &token),
        api::fetch_artifacts(&client, &token),
    );

    let logs = logs_res?;
    let packages = packages_res?;
    let artifacts = artifacts_res?;

    let (r1, r2, r3) = tokio::join!(
        db::insert_logs(pool, logs),
        db::insert_packages(pool, packages.clone()),
        db::insert_artifacts(pool, artifacts.clone()),
    );
    r1?;
    r2?;
    r3?;

    let error_futs: Vec<_> = artifacts
        .iter()
        .filter(|a| a.status.as_deref() == Some("ERROR"))
        .filter_map(|a| a.id.clone())
        .map(|id| {
            let client = client.clone();
            let token = token.clone();
            let pool = pool.clone();
            async move {
                if let Ok(Some(err_txt)) = api::fetch_artifact_error(&client, &token, &id).await {
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
        })
        .collect();
    futures::future::join_all(error_futs).await;

    let pkg_ids: Vec<String> = packages.iter().filter_map(|p| p.id.clone()).collect();

    let mapping = api::fetch_all_package_artifacts(&client, &token, &pkg_ids).await;

    let pairs: Vec<(String, String)> = mapping
        .iter()
        .flat_map(|(pkg_id, art_ids)| art_ids.iter().map(move |aid| (pkg_id.clone(), aid.clone())))
        .collect();

    db::bulk_update_artifact_package(pool, &pairs).await?;

    let all_art_ids: Vec<String> = mapping.into_values().flatten().collect();

    let config_stream = stream::iter(all_art_ids).map(|art_id| {
        let client = client.clone();
        let token = token.clone();
        let pool = pool.clone();
        async move {
            if let Ok(configs) = api::fetch_artifact_properties(&client, &token, &art_id).await {
                let _ = db::insert_configurations(&pool, &art_id, configs).await;
            }
        }
    });

    config_stream.buffer_unordered(10).collect::<Vec<_>>().await;

    queries::refresh_all(app, pool).await?;

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();

    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL manquante dans .env");

    // Pool PostgreSQL avec paramètres adaptés à la charge
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(8)
        .connect(&db_url)
        .await?;

    // ── Mode health-check ─────────────────────────────────────────────────────
    if cli.health {
        let pb = utils::create_spinner("Vérification des connexions...");
        let client = api::build_http_client()?;
        api::get_sap_token(&client).await?;
        pb.finish_with_message("✓ SAP OK  ✓ DB OK — Système opérationnel");
        return Ok(());
    }

    // ── Chargement initial ────────────────────────────────────────────────────
    let top = cli.top.unwrap_or(200);
    let mut app = App::new();

    {
        let pb = utils::create_spinner(&format!("Chargement initial ({} logs)...", top));
        match load_data(&mut app, &pool, top).await {
            Ok(()) => pb.finish_with_message("Données chargées !"),
            Err(e) => {
                pb.finish_with_message(format!("Erreur chargement : {}", e));
                // On tente quand même de charger ce qui est en base
                let _ = queries::refresh_all(&mut app, &pool).await;
            }
        }
    }

    // ── Boucle principale ─────────────────────────────────────────────────────
    const AUTO_REFRESH: Duration = Duration::from_secs(300);

    loop {
        let event = ui::run_tui(&mut app)?;

        match event {
            AppEvent::Quit => break,

            AppEvent::Refresh => {
                let limit = app.logs_limit;
                let pb = utils::create_spinner("Re-extraction SAP...");
                match load_data(&mut app, &pool, limit).await {
                    Ok(()) => {
                        pb.finish_with_message("Re-extraction terminée !");
                        app.overlay = ui::OverlayState::Done {
                            message: "Données mises à jour avec succès.".to_string(),
                        };
                    }
                    Err(e) => {
                        pb.finish_with_message("Erreur lors de l'extraction.");
                        app.overlay = ui::OverlayState::Error {
                            message: format!("{}", e),
                        };
                    }
                }
            }

            AppEvent::LoadMore => {
                let limit = app.logs_limit;
                let pb = utils::create_spinner(&format!("Chargement de {} logs...", limit));
                match load_data(&mut app, &pool, limit).await {
                    Ok(()) => pb.finish_with_message("Chargement terminé !"),
                    Err(e) => pb.finish_with_message(format!("Erreur: {}", e)),
                }
            }

            AppEvent::Continue => {
                // Auto-refresh si le délai est dépassé
                if app.last_refresh.elapsed() >= AUTO_REFRESH {
                    let limit = app.logs_limit;
                    let _ = load_data(&mut app, &pool, limit).await;
                }
            }
        }
    }

    println!("SAP BTP Monitor — Au revoir !");
    Ok(())
}
