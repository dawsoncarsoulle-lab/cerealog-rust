mod api;
mod config;
mod crypto;
mod db;
mod models;
mod queries;
mod tenant_setup;
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
    #[arg(long)]
    benchmark: bool,
    #[arg(long)]
    add_tenant: bool,
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
        match db::get_latest_log_date(&pool, &sap_config.tenant_id).await? {
            Some(last_date) => {
                let since = last_date.format("%Y-%m-%dT%H:%M:%S").to_string();
                log::info!(
                    "[{}] Fetch incrémental depuis {}",
                    sap_config.tenant_id,
                    since
                );
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
        log::info!("[{}] {} logs à insérer.", sap_config.tenant_id, logs.len());
        db::insert_logs(&pool, &sap_config.tenant_id, logs).await?;
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

    let error_ids: Vec<String> = artifacts
        .iter()
        .filter(|a| a.status.as_deref() == Some("ERROR"))
        .filter_map(|a| a.id.clone())
        .collect();

    let (r1, r2) = tokio::join!(
        db::insert_packages(&pool, &sap_config.tenant_id, packages),
        db::insert_artifacts(&pool, &sap_config.tenant_id, artifacts),
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
                        &sap_config.tenant_id,
                        models::ArtifactError {
                            artifact_id: id.clone(),
                            error_message: err_txt,
                            error_time: chrono::Utc::now().naive_utc(),
                        },
                    )
                    .await;
                    let _ = db::insert_pending_alert(
                        &pool,
                        &sap_config.tenant_id,
                        &id,
                        &id,
                        "deploy",
                        &snippet,
                    )
                    .await;
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
                    if let Err(e) =
                        db::insert_configurations(&pool, &sap_config.tenant_id, &art_id, cfgs).await
                    {
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

// ─── Worker de fetch (un par tenant) ─────────────────────────────────────────

async fn run_fetch_worker(
    pool: sqlx::PgPool,
    sap_config: SapConfig,
    top: u32,
    incremental: bool,
    tx: mpsc::Sender<RefreshData>,
) {
    let tenant_id = sap_config.tenant_id.clone();

    let send_fallback = |tx: mpsc::Sender<RefreshData>, pool: sqlx::PgPool| async move {
        let data = queries::build_refresh_data(&pool, top)
            .await
            .unwrap_or_else(|_| RefreshData {
                logs: vec![],
                exec_errors: vec![],
                active_exec_errors: vec![],
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

    // Chaque worker gère son token en mémoire (pas de cache disque partagé)
    let client = match api::build_http_client() {
        Ok(c) => c,
        Err(e) => {
            log::error!("[{}] HTTP client: {}", tenant_id, e);
            send_fallback(tx, pool).await;
            return;
        }
    };

    let mut token_cache = match api::get_sap_token(&client, &sap_config).await {
        Ok(t) => t,
        Err(e) => {
            log::error!("[{}] Auth worker: {}", tenant_id, e);
            send_fallback(tx, pool).await;
            return;
        }
    };

    if let Err(e) = api::ensure_valid_token(&client, &sap_config, &mut token_cache).await {
        log::error!("[{}] ensure_valid_token: {}", tenant_id, e);
        send_fallback(tx, pool).await;
        return;
    }

    let token = token_cache.get().to_string();
    let log_concurrency = 50usize;
    let concurrency = 20usize;

    if incremental {
        let (logs_res, artifacts_res) = tokio::join!(
            sync_logs(
                client.clone(),
                token.clone(),
                sap_config.clone(),
                pool.clone(),
                top,
                true,
                log_concurrency
            ),
            api::fetch_artifacts(&client, &token, &sap_config)
        );

        if let Err(e) = logs_res {
            log::error!("[{}] sync_logs échoué: {}", tenant_id, e);
        }

        if let Ok(artifacts) = artifacts_res {
            let _ = db::insert_artifacts(&pool, &tenant_id, artifacts.clone()).await;
            for a in artifacts {
                if a.status.as_deref() == Some("ERROR") {
                    if let Some(id) = a.id {
                        let c = client.clone();
                        let t = token.clone();
                        let sc = sap_config.clone();
                        let p = pool.clone();
                        let t_id = sap_config.tenant_id.clone();

                        tokio::spawn(async move {
                            if let Ok(Some(err)) = api::fetch_artifact_error(&c, &t, &sc, &id).await
                            {
                                let snippet: String = err.chars().take(200).collect();

                                let _ = db::insert_artifact_error(
                                    &p,
                                    &t_id,
                                    models::ArtifactError {
                                        artifact_id: id.clone(),
                                        error_message: err,
                                        error_time: chrono::Utc::now().naive_utc(),
                                    },
                                )
                                .await;

                                let _ = db::insert_pending_alert(
                                    &p, &t_id, &id, &id, "deploy", &snippet,
                                )
                                .await;
                            }
                        });
                    }
                }
            }
        }
    } else {
        let (logs_res, packages_res) = tokio::join!(
            sync_logs(
                client.clone(),
                token.clone(),
                sap_config.clone(),
                pool.clone(),
                top,
                false,
                log_concurrency
            ),
            sync_packages_and_artifacts(
                client.clone(),
                token.clone(),
                sap_config.clone(),
                pool.clone(),
                concurrency
            )
        );

        if let Err(e) = logs_res {
            log::error!("[{}] sync_logs échoué: {}", tenant_id, e);
        }

        let pkg_ids = match packages_res {
            Ok(ids) => ids,
            Err(e) => {
                log::error!("[{}] sync_packages échoué: {}", tenant_id, e);
                Vec::new()
            }
        };

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
                log::warn!("[{}] sync_configs: {}", tenant_id, e);
            }
        }
    }

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
                let _ = db::insert_pending_alert(
                    &pool,
                    &sap_config.tenant_id,
                    guid,
                    flow,
                    "exec",
                    &snippet,
                )
                .await;
            }
        }
    }

    send_fallback(tx, pool).await;
}

// ─── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();

    if cli.benchmark {
        println!("benchmark désactivé dans cette version");
        return Ok(());
    }

    env_logger::init();

    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL manquante dans .env");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(8)
        .connect(&db_url)
        .await?;

    if cli.add_tenant {
        tenant_setup::add_tenant_interactive(&pool).await?;
        return Ok(());
    }

    db::ensure_pending_alerts_table(&pool).await?;
    db::ensure_smart_alerts_table(&pool).await?;

    let configs = SapConfig::load_all_from_db(&pool).await?;
    if configs.is_empty() {
        anyhow::bail!("Aucun tenant actif trouvé en BDD. Lance `--add-tenant` pour en ajouter un.");
    }
    log::info!(
        "{} tenant(s) chargé(s) : {}",
        configs.len(),
        configs
            .iter()
            .map(|c| c.tenant_id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );

    // ── Health check
    if cli.health {
        let pb = utils::create_spinner("Vérification des connexions...");
        let client = api::build_http_client()?;
        for cfg in &configs {
            api::get_sap_token(&client, cfg).await?;
            log::info!("✓ Tenant '{}' OK", cfg.tenant_id);
        }
        pb.finish_with_message(format!("✓ {} tenant(s) OK  ✓ DB OK", configs.len()));
        return Ok(());
    }

    let user_config = UserConfig::load();
    let top = cli.top.unwrap_or(user_config.logs_limit);
    let mut app = App::new(user_config);

    let (tx, mut rx) = mpsc::channel::<RefreshData>(configs.len() * 2 + 2);

    // ── Spawner un worker par tenant
    for cfg in &configs {
        let incremental = !cli.full
            && db::get_latest_log_date(&pool, &cfg.tenant_id)
                .await?
                .is_some();
        app.refreshing = true;
        tokio::spawn(run_fetch_worker(
            pool.clone(),
            cfg.clone(),
            top,
            incremental,
            tx.clone(),
        ));
    }

    // ── Webhook worker
    {
        let pool_w = pool.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                if let Err(e) = webhook::process_pending_alerts(&pool_w).await {
                    log::warn!("Webhook worker: {}", e);
                }
                if let Err(e) = webhook::process_smart_alerts(&pool_w).await {
                    log::warn!("Smart alert worker: {}", e);
                }
            }
        });
    }

    // ── Boucle UI
    let configs_arc = std::sync::Arc::new(configs);
    loop {
        let event = ui::run_tui(&mut app, &mut rx)?;

        match event {
            AppEvent::Quit => {
                app.user_config.logs_limit = app.logs_limit;
                app.user_config.save();
                break;
            }
            AppEvent::TriggerRefresh { full } => {
                for cfg in configs_arc.iter() {
                    tokio::spawn(run_fetch_worker(
                        pool.clone(),
                        cfg.clone(),
                        app.logs_limit,
                        !full,
                        tx.clone(),
                    ));
                }
            }
            AppEvent::LoadMore => {
                for cfg in configs_arc.iter() {
                    tokio::spawn(run_fetch_worker(
                        pool.clone(),
                        cfg.clone(),
                        app.logs_limit,
                        false,
                        tx.clone(),
                    ));
                }
            }
            AppEvent::Continue => {
                for cfg in configs_arc.iter() {
                    tokio::spawn(run_fetch_worker(
                        pool.clone(),
                        cfg.clone(),
                        app.logs_limit,
                        true,
                        tx.clone(),
                    ));
                }
            }
        }
    }

    println!("SAP BTP Monitor — Au revoir !");
    Ok(())
}
