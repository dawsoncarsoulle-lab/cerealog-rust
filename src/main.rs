mod api;
mod config;
mod db;
mod models;
mod queries;
mod ui;
mod utils;
mod webhook;

use clap::Parser;
use config::{SapConfig, UserConfig};
use models::RefreshData;
use tokio::sync::mpsc;
use ui::{App, AppEvent};

#[derive(Parser, Debug)]
#[command(author, version, about = "SAP BTP Monitor — Dashboard TUI")]
struct Cli {
    #[arg(short, long)]
    top: Option<u32>,
    #[arg(long)]
    health: bool,
    #[arg(long)]
    full: bool,
}

// ─── Synchronisation logs ─────────────────────────────────────────────────────

async fn sync_logs(
    client: reqwest::Client,
    token: String,
    sap_config: SapConfig,
    pool: sqlx::PgPool,
    top: u32,
    incremental: bool,
    concurrency: usize,
) -> anyhow::Result<()> {
    let filter = if incremental {
        match db::get_latest_log_date(&pool).await? {
            Some(last_date) => {
                let since = last_date.format("%Y-%m-%dT%H:%M:%S").to_string();
                log::info!("Fetch incrémental depuis {}", since);
                Some(format!("LogStart gt datetime'{}'", since))
            }
            None => None,
        }
    } else {
        None
    };

    let logs = api::fetch_sap_logs(
        &client,
        &token,
        &sap_config,
        top,
        filter.as_deref(),
        concurrency,
    )
    .await?;
    if !logs.is_empty() {
        log::info!("{} logs à insérer.", logs.len());
        db::insert_logs(&pool, logs).await?;
    }
    Ok(())
}

// ─── Synchronisation packages + artifacts ─────────────────────────────────────

async fn sync_packages_and_artifacts(
    client: reqwest::Client,
    token: String,
    sap_config: SapConfig,
    pool: sqlx::PgPool,
    concurrency: usize,
) -> anyhow::Result<Vec<String>> {
    let (packages_res, artifacts_res) = tokio::join!(
        api::fetch_packages(&client, &token, &sap_config),
        api::fetch_artifacts(&client, &token, &sap_config),
    );

    let packages = packages_res?;
    let artifacts = artifacts_res?;
    let pkg_ids: Vec<String> = packages.iter().filter_map(|p| p.id.clone()).collect();

    // IDs en erreur matérialisés AVANT les futures (évite capture de &artifacts)
    let error_ids: Vec<String> = artifacts
        .iter()
        .filter(|a| a.status.as_deref() == Some("ERROR"))
        .filter_map(|a| a.id.clone())
        .collect();

    let (r1, r2) = tokio::join!(
        db::insert_packages(&pool, packages),
        db::insert_artifacts(&pool, artifacts),
    );
    r1?;
    r2?;

    let mut handles = Vec::new();
    for id in error_ids {
        let client = client.clone();
        let token = token.clone();
        let sap_config = sap_config.clone();
        let pool = pool.clone();
        handles.push(tokio::spawn(async move {
            match api::fetch_artifact_error(&client, &token, &sap_config, &id).await {
                Ok(Some(err_txt)) => {
                    let snippet: String = err_txt.chars().take(200).collect();
                    let _ = db::insert_artifact_error(
                        &pool,
                        models::ArtifactError {
                            artifact_id: id.clone(),
                            error_message: err_txt,
                            error_time: chrono::Utc::now().naive_utc(),
                        },
                    )
                    .await;
                    let _ = db::insert_pending_alert(&pool, &id, &id, "deploy", &snippet).await;
                }
                Ok(None) => {}
                Err(e) => log::warn!("Fetch erreur artifact {}: {}", id, e),
            }
        }));
        if handles.len() >= concurrency {
            for h in handles.drain(..) {
                let _ = h.await;
            }
        }
    }
    for h in handles {
        let _ = h.await;
    }

    Ok(pkg_ids)
}

// ─── Synchronisation configurations ──────────────────────────────────────────

async fn sync_configurations(
    client: reqwest::Client,
    token: String,
    sap_config: SapConfig,
    pool: sqlx::PgPool,
    pkg_ids: &[String],
    concurrency: usize,
) -> anyhow::Result<()> {
    let mapping =
        api::fetch_all_package_artifacts(&client, &token, &sap_config, pkg_ids, concurrency).await;

    let pairs: Vec<(String, String)> = mapping
        .iter()
        .flat_map(|(pkg_id, art_ids)| art_ids.iter().map(move |aid| (pkg_id.clone(), aid.clone())))
        .collect();
    db::bulk_update_artifact_package(&pool, &pairs).await?;

    let all_art_ids: Vec<String> = mapping.into_values().flatten().collect();

    let mut handles = Vec::new();
    for art_id in all_art_ids {
        let client = client.clone();
        let token = token.clone();
        let sap_config = sap_config.clone();
        let pool = pool.clone();
        handles.push(tokio::spawn(async move {
            match api::fetch_artifact_properties(&client, &token, &sap_config, &art_id).await {
                Ok(cfgs) => {
                    if let Err(e) = db::insert_configurations(&pool, &art_id, cfgs).await {
                        log::warn!("Insert configs {} échoué: {}", art_id, e);
                    }
                }
                Err(e) => log::warn!("Fetch configs {} échoué: {}", art_id, e),
            }
        }));
        if handles.len() >= concurrency {
            for h in handles.drain(..) {
                let _ = h.await;
            }
        }
    }
    for h in handles {
        let _ = h.await;
    }

    Ok(())
}

// ─── Worker de fetch ('static + Send) ────────────────────────────────────────

async fn run_fetch_worker(
    pool: sqlx::PgPool,
    sap_config: SapConfig,
    top: u32,
    incremental: bool,
    tx: mpsc::Sender<RefreshData>,
) {
    // 💡 Astuce : Cette fonction locale garantit qu'on prévient toujours l'UI
    // de la fin du processus, même si le réseau plante, pour stopper le spinner.
    let send_fallback = |tx: mpsc::Sender<RefreshData>, pool: sqlx::PgPool| async move {
        let data = queries::build_refresh_data(&pool, top)
            .await
            .unwrap_or_else(|_| RefreshData {
                logs: vec![],
                exec_errors: vec![],
                artifacts: vec![],
                packages: vec![],
                deploy_errors: vec![],
                configs: std::collections::HashMap::new(),
                stats: Default::default(),
                error_sparkline: vec![],
                error_barchart: vec![],
                activity_sparkline: vec![],
                top_errors_barchart: vec![],
                status_counts: vec![],
            });
        let _ = tx.send(data).await;
    };

    let client = match api::build_http_client() {
        Ok(c) => c,
        Err(e) => {
            log::error!("HTTP client: {}", e);
            send_fallback(tx, pool).await;
            return;
        }
    };

    let mut token_cache = match api::get_sap_token(&client, &sap_config).await {
        Ok(t) => t,
        Err(e) => {
            log::error!("Auth worker: {}", e);
            send_fallback(tx, pool).await;
            return;
        }
    };

    if let Err(e) = api::ensure_valid_token(&client, &sap_config, &mut token_cache).await {
        log::error!("ensure_valid_token: {}", e);
        send_fallback(tx, pool).await;
        return;
    }
    let token = token_cache.get().to_string();
    let concurrency = 20usize;

    // Étape 1 : Logs
    if let Err(e) = sync_logs(
        client.clone(),
        token.clone(),
        sap_config.clone(),
        pool.clone(),
        top,
        incremental,
        concurrency,
    )
    .await
    {
        log::error!("sync_logs échoué: {}", e);
    }

    // Pending alerts exec
    if let Ok(failed) = queries::fetch_exec_errors(&pool).await {
        for l in &failed {
            if let (Some(guid), Some(flow)) = (&l.message_guid, &l.integration_flow_name) {
                let snippet = l
                    .error_message
                    .as_deref()
                    .unwrap_or("")
                    .chars()
                    .take(200)
                    .collect::<String>();
                let _ = db::insert_pending_alert(&pool, guid, flow, "exec", &snippet).await;
            }
        }
    }

    // Étape 2 : Packages + Artifacts
    let pkg_ids = match sync_packages_and_artifacts(
        client.clone(),
        token.clone(),
        sap_config.clone(),
        pool.clone(),
        concurrency,
    )
    .await
    {
        Ok(ids) => ids,
        Err(e) => {
            log::error!("sync_packages échoué: {}", e);
            Vec::new()
        }
    };

    // Étape 3 : Configurations
    if !pkg_ids.is_empty() {
        if let Err(e) = sync_configurations(
            client.clone(),
            token.clone(),
            sap_config.clone(),
            pool.clone(),
            &pkg_ids,
            concurrency,
        )
        .await
        {
            log::warn!("sync_configs: {}", e);
        }
    }

    // Étape 4 : Payload → UI (DÉBLOQUE LE SPINNER DANS TOUS LES CAS)
    send_fallback(tx, pool).await;
}

// ─── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    env_logger::init();

    let cli = Cli::parse();

    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL manquante dans .env");
    let sap_config = SapConfig::from_env()?;
    let user_config = UserConfig::load();

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(8)
        .connect(&db_url)
        .await?;

    db::ensure_pending_alerts_table(&pool).await?;

    if cli.health {
        let pb = utils::create_spinner("Vérification des connexions...");
        let client = api::build_http_client()?;
        api::get_sap_token(&client, &sap_config).await?;
        pb.finish_with_message("✓ SAP OK  ✓ DB OK");
        return Ok(());
    }

    let top = cli.top.unwrap_or(user_config.logs_limit);
    let mut app = App::new(user_config);

    let (tx, mut rx) = mpsc::channel::<RefreshData>(4);

    // Chargement initial en arrière-plan
    {
        let incremental = !cli.full && db::get_latest_log_date(&pool).await?.is_some();
        app.refreshing = true;
        tokio::spawn(run_fetch_worker(
            pool.clone(),
            sap_config.clone(),
            top,
            incremental,
            tx.clone(),
        ));
    }

    // Worker webhook toutes les 60s
    {
        let pool_w = pool.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                if let Err(e) = webhook::process_pending_alerts(&pool_w).await {
                    log::warn!("Webhook worker: {}", e);
                }
            }
        });
    }

    // Boucle principale
    loop {
        let event = ui::run_tui(&mut app, &mut rx)?;

        // ✅ CRITIQUE : Plus de vérifications 'if !app.refreshing' ici.
        // L'interface (ui.rs) gère déjà l'état et prévient les lancements en double.
        // Quand main.rs reçoit un événement, il l'exécute, point final.
        match event {
            AppEvent::Quit => {
                app.user_config.logs_limit = app.logs_limit;
                app.user_config.save();
                break;
            }
            AppEvent::TriggerRefresh { full } => {
                tokio::spawn(run_fetch_worker(
                    pool.clone(),
                    sap_config.clone(),
                    app.logs_limit,
                    !full,
                    tx.clone(),
                ));
            }
            AppEvent::LoadMore => {
                tokio::spawn(run_fetch_worker(
                    pool.clone(),
                    sap_config.clone(),
                    app.logs_limit,
                    false,
                    tx.clone(),
                ));
            }
            AppEvent::Continue => {
                tokio::spawn(run_fetch_worker(
                    pool.clone(),
                    sap_config.clone(),
                    app.logs_limit,
                    true,
                    tx.clone(),
                ));
            }
        }
    }

    println!("SAP BTP Monitor — Au revoir !");
    Ok(())
}
