mod api;
mod config;
mod db;
mod models;
mod queries;
mod ui;
mod utils;

use clap::Parser;
use config::{SapConfig, UserConfig};
use std::time::Duration;
use ui::{App, AppEvent};

#[derive(Parser, Debug)]
#[command(author, version, about = "SAP BTP Monitor — Dashboard TUI")]
struct Cli {
    #[arg(short, long, help = "Nombre de logs à récupérer au démarrage")]
    top: Option<u32>,
    #[arg(long, help = "Vérifier les connexions et quitter")]
    health: bool,
    #[arg(long, help = "Forcer un fetch complet (ignore le fetch incrémental)")]
    full: bool,
}

// ─── Étapes de synchronisation ───────────────────────────────────────────────

async fn sync_logs(
    client: &reqwest::Client,
    token: &str,
    config: &SapConfig,
    pool: &sqlx::PgPool,
    top: u32,
    incremental: bool,
    concurrency: usize,
) -> anyhow::Result<()> {
    let filter = if incremental {
        match db::get_latest_log_date(pool).await? {
            Some(last_date) => {
                let since = last_date.format("%Y-%m-%dT%H:%M:%S").to_string();
                log::info!("Fetch incrémental depuis {}", since);
                Some(format!("LogStart gt datetime'{}'", since))
            }
            None => {
                log::info!("Aucun log en base — fetch complet");
                None
            }
        }
    } else {
        None
    };

    let logs =
        api::fetch_sap_logs(client, token, config, top, filter.as_deref(), concurrency).await?;

    if logs.is_empty() {
        log::info!("Aucun nouveau log à insérer.");
        return Ok(());
    }

    log::info!("{} logs à insérer.", logs.len());
    db::insert_logs(pool, logs).await?;
    Ok(())
}

async fn sync_packages_and_artifacts(
    client: &reqwest::Client,
    token: &str,
    config: &SapConfig,
    pool: &sqlx::PgPool,
    concurrency: usize,
) -> anyhow::Result<Vec<String>> {
    use futures::stream::{self, StreamExt};

    let (packages_res, artifacts_res) = tokio::join!(
        api::fetch_packages(client, token, config),
        api::fetch_artifacts(client, token, config),
    );

    let packages = packages_res?;
    let artifacts = artifacts_res?;

    // Extraire les IDs avant de consommer les vecs
    let pkg_ids: Vec<String> = packages.iter().filter_map(|p| p.id.clone()).collect();

    let (r1, r2) = tokio::join!(
        db::insert_packages(pool, packages),
        db::insert_artifacts(pool, artifacts.clone()),
    );
    r1?;
    r2?;

    // Errors des artifacts en échec
    let error_stream = stream::iter(
        artifacts
            .iter()
            .filter(|a| a.status.as_deref() == Some("ERROR"))
            .filter_map(|a| a.id.clone())
            .map(|id| {
                let client = client.clone();
                let token = token.to_string();
                let pool = pool.clone();
                let config = config.clone();
                async move {
                    match api::fetch_artifact_error(&client, &token, &config, &id).await {
                        Ok(Some(err_txt)) => {
                            if let Err(e) = db::insert_artifact_error(
                                &pool,
                                models::ArtifactError {
                                    artifact_id: id.clone(),
                                    error_message: err_txt,
                                    error_time: chrono::Utc::now().naive_utc(),
                                },
                            )
                            .await
                            {
                                log::warn!("Impossible d'insérer l'erreur pour {}: {}", id, e);
                            }
                        }
                        Ok(None) => {}
                        Err(e) => {
                            log::warn!("Fetch erreur artifact {} échoué: {}", id, e);
                        }
                    }
                }
            }),
    );
    error_stream
        .buffer_unordered(concurrency)
        .collect::<Vec<_>>()
        .await;

    Ok(pkg_ids)
}

async fn sync_configurations(
    client: &reqwest::Client,
    token: &str,
    config: &SapConfig,
    pool: &sqlx::PgPool,
    pkg_ids: &[String],
    concurrency: usize,
) -> anyhow::Result<()> {
    use futures::stream::{self, StreamExt};

    let mapping =
        api::fetch_all_package_artifacts(client, token, config, pkg_ids, concurrency).await;

    let pairs: Vec<(String, String)> = mapping
        .iter()
        .flat_map(|(pkg_id, art_ids)| art_ids.iter().map(move |aid| (pkg_id.clone(), aid.clone())))
        .collect();

    db::bulk_update_artifact_package(pool, &pairs).await?;

    let all_art_ids: Vec<String> = mapping.into_values().flatten().collect();

    let config_stream = stream::iter(all_art_ids).map(|art_id| {
        let client = client.clone();
        let token = token.to_string();
        let pool = pool.clone();
        let sap_config = config.clone();
        async move {
            match api::fetch_artifact_properties(&client, &token, &sap_config, &art_id).await {
                Ok(configs) => {
                    if let Err(e) = db::insert_configurations(&pool, &art_id, configs).await {
                        log::warn!("Insert configs pour {} échoué: {}", art_id, e);
                    }
                }
                Err(e) => log::warn!("Fetch configs pour {} échoué: {}", art_id, e),
            }
        }
    });
    config_stream
        .buffer_unordered(concurrency)
        .collect::<Vec<_>>()
        .await;

    Ok(())
}

/// Point d'entrée principal du chargement — orchestre les 3 étapes.
async fn load_data(
    app: &mut App,
    pool: &sqlx::PgPool,
    sap_config: &SapConfig,
    token_cache: &mut config::TokenCache,
    top: u32,
    incremental: bool,
) -> anyhow::Result<()> {
    let client = api::build_http_client()?;
    let concurrency = app.user_config.parallel_requests;

    // Renouveler le token si nécessaire
    api::ensure_valid_token(&client, sap_config, token_cache).await?;
    let token = token_cache.get().to_string();

    // Étape 1 : Logs (fetch incrémental ou complet)
    if let Err(e) = sync_logs(
        &client,
        &token,
        sap_config,
        pool,
        top,
        incremental,
        concurrency,
    )
    .await
    {
        log::error!("Sync logs échouée: {}", e);
        return Err(e);
    }

    // Étape 2 : Packages, artifacts et leurs erreurs
    let pkg_ids =
        sync_packages_and_artifacts(&client, &token, sap_config, pool, concurrency).await?;

    // Étape 3 : Configurations des artifacts (peut se faire en arrière-plan)
    if let Err(e) =
        sync_configurations(&client, &token, sap_config, pool, &pkg_ids, concurrency).await
    {
        log::warn!("Sync configs échouée (non bloquant): {}", e);
    }

    queries::refresh_all(app, pool).await?;
    Ok(())
}

// ─── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    env_logger::init();

    let args: Vec<String> = std::env::args().collect();
    if args.contains(&"--daemon".to_string()) {
        println!("🚀 Mode Démon activé : Test de performance en cours...");

        let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL manquante dans .env");
        let sap_config = SapConfig::from_env()?;

        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(8)
            .connect(&db_url)
            .await?;

        let client = api::build_http_client()?;
        let mut token_cache = api::get_sap_token(&client, &sap_config).await?;

        // On cherche si on a passé l'argument --top (ex: --top 500)
        let mut top = 500; // Valeur par défaut
        if let Some(pos) = args.iter().position(|a| a == "--top") {
            if let Some(val) = args.get(pos + 1) {
                if let Ok(parsed_top) = val.parse::<u32>() {
                    top = parsed_top;
                }
            }
        }

        // ✅ MATCH ÉQUITABLE : On extrait et insère UNIQUEMENT les logs, comme le script Python
        println!("⏬ Téléchargement et insertion de {} logs...", top);
        sync_logs(
            &client,
            token_cache.get(),
            &sap_config,
            &pool,
            top,
            false,
            20,
        )
        .await?;

        println!("✅ Extraction terminée.");
        return Ok(());
    }
    // -------------------------------------------------------------------

    // Le reste de ton programme normal (TUI)
    let cli = Cli::parse();

    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL manquante dans .env");
    let sap_config = SapConfig::from_env()?;
    let user_config = UserConfig::load();

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(8)
        .connect(&db_url)
        .await?;

    // ── Mode health-check ─────────────────────────────────────────────────────
    if cli.health {
        let pb = utils::create_spinner("Vérification des connexions...");
        let client = api::build_http_client()?;
        api::get_sap_token(&client, &sap_config).await?;
        pb.finish_with_message("✓ SAP OK  ✓ DB OK — Système opérationnel");
        return Ok(());
    }

    // ── Initialisation ────────────────────────────────────────────────────────
    let top = cli.top.unwrap_or(user_config.logs_limit);
    let mut app = App::new(user_config);

    // Obtenir le token initial
    let client = api::build_http_client()?;
    let mut token_cache = {
        let pb = utils::create_spinner("Authentification SAP...");
        let cache = api::get_sap_token(&client, &sap_config).await?;
        pb.finish_with_message("✓ Token obtenu");
        cache
    };

    // ── Chargement initial ────────────────────────────────────────────────────
    {
        let pb = utils::create_spinner(&format!("Chargement initial ({} logs)...", top));
        // Premier chargement = fetch complet pour remplir la base
        let incremental = !cli.full && db::get_latest_log_date(&pool).await?.is_some();
        match load_data(
            &mut app,
            &pool,
            &sap_config,
            &mut token_cache,
            top,
            incremental,
        )
        .await
        {
            Ok(()) => pb.finish_with_message("Données chargées !"),
            Err(e) => {
                pb.finish_with_message(format!("Erreur chargement : {}", e));
                let _ = queries::refresh_all(&mut app, &pool).await;
            }
        }
    }

    // ── Boucle principale ─────────────────────────────────────────────────────
    const AUTO_REFRESH: std::time::Duration = std::time::Duration::from_secs(300);

    loop {
        let event = ui::run_tui(&mut app)?;

        match event {
            AppEvent::Quit => {
                // Sauvegarder la config utilisateur à la sortie
                app.user_config.logs_limit = app.logs_limit;
                app.user_config.save();
                break;
            }

            AppEvent::Refresh => {
                let limit = app.logs_limit;
                let pb = utils::create_spinner("Re-extraction SAP (fetch complet)...");
                // Refresh manuel = fetch complet pour récupérer tout
                match load_data(&mut app, &pool, &sap_config, &mut token_cache, limit, false).await
                {
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
                match load_data(&mut app, &pool, &sap_config, &mut token_cache, limit, false).await
                {
                    Ok(()) => pb.finish_with_message("Chargement terminé !"),
                    Err(e) => pb.finish_with_message(format!("Erreur: {}", e)),
                }
            }

            AppEvent::Continue => {
                if app.last_refresh.elapsed() >= AUTO_REFRESH {
                    let limit = app.logs_limit;
                    // Auto-refresh = fetch incrémental pour être rapide
                    if let Err(e) =
                        load_data(&mut app, &pool, &sap_config, &mut token_cache, limit, true).await
                    {
                        log::warn!("Auto-refresh échoué: {}", e);
                    }
                }
            }
        }
    }

    println!("SAP BTP Monitor — Au revoir !");
    Ok(())
}
