mod api;
mod config;
mod crypto;
mod db;
mod models;
mod queries;
mod tenant_setup;
mod webhook;

use anyhow::{Context, Result};
use clap::Parser;
use config::{SapConfig, TokenCache};
use futures::stream::{self, StreamExt};
use models::{ArtifactError, LogEntry};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

#[derive(Parser, Debug, Clone)]
#[command(
    author,
    version,
    about = "SAP BTP Monitor — backend daemon/batch worker"
)]
struct Cli {
    /// Nombre maximum de logs à lire par tenant et par cycle.
    #[arg(short, long, default_value_t = 1_000)]
    top: u32,

    /// Taille d'une page OData MessageProcessingLogs.
    #[arg(long, default_value_t = 1_000)]
    page_size: u32,

    /// Force une resynchronisation sans filtre incrémental.
    #[arg(long)]
    full: bool,

    /// Exécute un cycle puis quitte. C'est le mode recommandé pour Airflow BashOperator.
    #[arg(long)]
    once: bool,

    /// Lance une boucle daemon avec intervalle fixe.
    #[arg(long)]
    daemon: bool,

    /// Intervalle entre deux cycles daemon.
    #[arg(long, default_value_t = 300)]
    interval_seconds: u64,

    /// Vérifie DB + authentification SAP puis quitte.
    #[arg(long)]
    health: bool,

    /// Ajoute ou met à jour un tenant en mode interactif.
    #[arg(long)]
    add_tenant: bool,

    /// Filtre un ou plusieurs tenants. Répéter l'option pour plusieurs tenants.
    #[arg(long = "tenant")]
    tenants: Vec<String>,

    /// Connexions max au pool PostgreSQL.
    #[arg(long, default_value_t = 16)]
    db_connections: u32,

    /// Requêtes concurrentes pour récupérer les ErrorInformation des MPL FAILED.
    #[arg(long, default_value_t = 100)]
    log_concurrency: usize,

    /// Requêtes concurrentes packages/artifacts/configurations.
    #[arg(long, default_value_t = 32)]
    metadata_concurrency: usize,

    /// Tenants synchronisés en parallèle. 0 = tous les tenants actifs.
    #[arg(long, default_value_t = 0)]
    tenant_concurrency: usize,

    /// Ne synchronise que les logs MPL.
    #[arg(long)]
    logs_only: bool,

    /// Ne synchronise que packages/artifacts/configurations.
    #[arg(long)]
    metadata_only: bool,

    /// Ignore les configurations d'artifacts.
    #[arg(long)]
    skip_configs: bool,

    /// Ignore Teams/webhook.
    #[arg(long)]
    skip_webhooks: bool,

    /// N'appelle pas ErrorInformation/$value pour chaque MPL FAILED.
    /// Utile pour des backfills massifs où le débit prime sur le détail.
    #[arg(long)]
    skip_error_details: bool,

    /// Nombre max de MPL FAILED du cycle convertis en pending_alerts.
    #[arg(long, default_value_t = 1_000)]
    alert_scan_limit: usize,

    /// Intervalle du worker webhook en mode daemon.
    #[arg(long, default_value_t = 60)]
    webhook_interval_seconds: u64,

    /// Timeout HTTP global par requête.
    #[arg(long, default_value_t = 60)]
    http_timeout_seconds: u64,

    /// Connexions HTTP idle gardées par host.
    #[arg(long, default_value_t = 200)]
    http_idle_per_host: usize,
}

#[derive(Clone)]
struct SyncOptions {
    top: u32,
    page_size: u32,
    full: bool,
    log_concurrency: usize,
    metadata_concurrency: usize,
    tenant_concurrency: usize,
    logs_only: bool,
    metadata_only: bool,
    skip_configs: bool,
    fetch_error_details: bool,
    alert_scan_limit: usize,
}

impl From<&Cli> for SyncOptions {
    fn from(cli: &Cli) -> Self {
        let top = cli.top.max(1);
        Self {
            top,
            page_size: cli.page_size.clamp(1, 5_000).min(top),
            full: cli.full,
            log_concurrency: cli.log_concurrency.max(1),
            metadata_concurrency: cli.metadata_concurrency.max(1),
            tenant_concurrency: cli.tenant_concurrency,
            logs_only: cli.logs_only,
            metadata_only: cli.metadata_only,
            skip_configs: cli.skip_configs,
            fetch_error_details: !cli.skip_error_details,
            alert_scan_limit: cli.alert_scan_limit,
        }
    }
}

#[derive(Clone)]
struct TenantRuntime {
    config: SapConfig,
    token: Arc<Mutex<TokenCache>>,
}

#[derive(Debug, Default)]
struct TenantReport {
    tenant_id: String,
    logs: usize,
    packages: usize,
    artifacts: usize,
    deploy_errors: usize,
    exec_alerts: usize,
    configs: usize,
    elapsed_ms: u128,
}

#[derive(Debug, Default)]
struct CycleSummary {
    reports: Vec<TenantReport>,
    errors: Vec<String>,
    elapsed_ms: u128,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    init_logger();

    let cli = Cli::parse();

    let db_url = std::env::var("DATABASE_URL").context("DATABASE_URL manquante")?;
    let pool = PgPoolOptions::new()
        .max_connections(cli.db_connections)
        .acquire_timeout(Duration::from_secs(30))
        .connect(&db_url)
        .await
        .context("Connexion PostgreSQL impossible")?;

    if cli.add_tenant {
        tenant_setup::add_tenant_interactive(&pool).await?;
        return Ok(());
    }

    prepare_database(&pool).await?;

    let mut configs = SapConfig::load_all_from_db(&pool).await?;
    if !cli.tenants.is_empty() {
        configs.retain(|cfg| cli.tenants.iter().any(|wanted| wanted == &cfg.tenant_id));
    }
    if configs.is_empty() {
        anyhow::bail!("Aucun tenant actif trouvé pour ce lancement.");
    }

    let client =
        api::build_http_client_with_options(cli.http_idle_per_host, cli.http_timeout_seconds)?;

    if cli.health {
        health_check(&client, &configs).await?;
        println!(
            "OK: DB connectée, {} tenant(s) SAP authentifié(s).",
            configs.len()
        );
        return Ok(());
    }

    let runtimes = init_tenant_runtimes(&client, configs).await?;
    let opts = SyncOptions::from(&cli);

    if cli.daemon && !cli.once {
        run_daemon(pool, client, runtimes, opts, &cli).await
    } else {
        let summary = run_cycle(&pool, &client, &runtimes, &opts).await;
        print_cycle_summary(&summary);
        if !cli.skip_webhooks {
            run_webhooks_once(&pool).await?;
        }
        if !summary.errors.is_empty() {
            anyhow::bail!(
                "{} tenant(s) en erreur pendant le cycle.",
                summary.errors.len()
            );
        }
        Ok(())
    }
}

fn init_logger() {
    let env = env_logger::Env::default().default_filter_or("info");
    env_logger::Builder::from_env(env).init();
}

async fn prepare_database(pool: &sqlx::PgPool) -> Result<()> {
    db::ensure_pending_alerts_table(pool).await?;
    db::ensure_smart_alerts_table(pool).await?;
    Ok(())
}

async fn health_check(client: &reqwest::Client, configs: &[SapConfig]) -> Result<()> {
    let results: Vec<Result<()>> = stream::iter(configs.iter().cloned())
        .map(|cfg| {
            let client = client.clone();
            async move {
                api::get_sap_token(&client, &cfg).await?;
                log::info!("✓ Tenant '{}' OK", cfg.tenant_id);
                Ok(())
            }
        })
        .buffer_unordered(16)
        .collect()
        .await;

    for res in results {
        res?;
    }
    Ok(())
}

async fn init_tenant_runtimes(
    client: &reqwest::Client,
    configs: Vec<SapConfig>,
) -> Result<Vec<TenantRuntime>> {
    let runtimes: Vec<Result<TenantRuntime>> = stream::iter(configs.into_iter())
        .map(|cfg| {
            let client = client.clone();
            async move {
                let token = api::get_sap_token(&client, &cfg).await?;
                Ok(TenantRuntime {
                    config: cfg,
                    token: Arc::new(Mutex::new(token)),
                })
            }
        })
        .buffer_unordered(16)
        .collect()
        .await;

    let mut out = Vec::with_capacity(runtimes.len());
    for runtime in runtimes {
        out.push(runtime?);
    }
    log::info!("{} tenant(s) initialisé(s).", out.len());
    Ok(out)
}

async fn run_daemon(
    pool: sqlx::PgPool,
    client: reqwest::Client,
    runtimes: Vec<TenantRuntime>,
    opts: SyncOptions,
    cli: &Cli,
) -> Result<()> {
    log::info!(
        "Mode daemon démarré: interval={}s, top={}, page_size={}, tenants={}",
        cli.interval_seconds,
        opts.top,
        opts.page_size,
        runtimes.len()
    );

    let webhook_handle = if cli.skip_webhooks {
        None
    } else {
        Some(spawn_webhook_worker(
            pool.clone(),
            Duration::from_secs(cli.webhook_interval_seconds.max(5)),
        ))
    };

    loop {
        let summary = run_cycle(&pool, &client, &runtimes, &opts).await;
        print_cycle_summary(&summary);

        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                log::info!("Signal d'arrêt reçu, fermeture du daemon.");
                break;
            }
            _ = tokio::time::sleep(Duration::from_secs(cli.interval_seconds.max(1))) => {}
        }
    }

    if let Some(handle) = webhook_handle {
        handle.abort();
    }

    Ok(())
}

fn spawn_webhook_worker(pool: sqlx::PgPool, interval: Duration) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(interval);
        loop {
            tick.tick().await;
            if let Err(e) = run_webhooks_once(&pool).await {
                log::warn!("Webhook worker: {}", e);
            }
        }
    })
}

async fn run_webhooks_once(pool: &sqlx::PgPool) -> Result<()> {
    webhook::process_pending_alerts(pool).await?;
    webhook::process_smart_alerts(pool).await?;
    Ok(())
}

async fn run_cycle(
    pool: &sqlx::PgPool,
    client: &reqwest::Client,
    runtimes: &[TenantRuntime],
    opts: &SyncOptions,
) -> CycleSummary {
    let started = Instant::now();
    let tenant_concurrency = if opts.tenant_concurrency == 0 {
        runtimes.len().max(1)
    } else {
        opts.tenant_concurrency.max(1).min(runtimes.len().max(1))
    };

    let outcomes: Vec<Result<TenantReport>> = stream::iter(runtimes.iter().cloned())
        .map(|runtime| {
            let pool = pool.clone();
            let client = client.clone();
            let opts = opts.clone();
            async move { sync_tenant(&pool, &client, runtime, &opts).await }
        })
        .buffer_unordered(tenant_concurrency)
        .collect()
        .await;

    let mut summary = CycleSummary {
        elapsed_ms: started.elapsed().as_millis(),
        ..Default::default()
    };

    for outcome in outcomes {
        match outcome {
            Ok(report) => summary.reports.push(report),
            Err(e) => summary.errors.push(format!("{:#}", e)),
        }
    }

    summary
}

async fn sync_tenant(
    pool: &sqlx::PgPool,
    client: &reqwest::Client,
    runtime: TenantRuntime,
    opts: &SyncOptions,
) -> Result<TenantReport> {
    let started = Instant::now();
    let tenant_id = runtime.config.tenant_id.clone();
    let token = get_valid_token(client, &runtime).await?;

    let mut report = TenantReport {
        tenant_id: tenant_id.clone(),
        ..Default::default()
    };

    if !opts.metadata_only {
        let incremental = !opts.full && db::get_latest_log_date(pool, &tenant_id).await?.is_some();
        let (logs, exec_alerts) =
            sync_logs(pool, client, &runtime.config, &token, opts, incremental).await?;
        report.logs = logs;
        report.exec_alerts = exec_alerts;
    }

    if !opts.logs_only {
        let metadata =
            sync_packages_artifacts_and_configs(pool, client, &runtime.config, &token, opts)
                .await?;
        report.packages = metadata.packages;
        report.artifacts = metadata.artifacts;
        report.deploy_errors = metadata.deploy_errors;
        report.configs = metadata.configs;
    }

    report.elapsed_ms = started.elapsed().as_millis();
    log::info!(
        "[{}] cycle OK: logs={} packages={} artifacts={} configs={} deploy_errors={} exec_alerts={} elapsed={}ms",
        report.tenant_id,
        report.logs,
        report.packages,
        report.artifacts,
        report.configs,
        report.deploy_errors,
        report.exec_alerts,
        report.elapsed_ms,
    );
    Ok(report)
}

async fn get_valid_token(client: &reqwest::Client, runtime: &TenantRuntime) -> Result<String> {
    let mut guard = runtime.token.lock().await;
    api::ensure_valid_token(client, &runtime.config, &mut guard).await?;
    Ok(guard.get().to_string())
}

async fn sync_logs(
    pool: &sqlx::PgPool,
    client: &reqwest::Client,
    sap_config: &SapConfig,
    token: &str,
    opts: &SyncOptions,
    incremental: bool,
) -> Result<(usize, usize)> {
    let filter = if incremental {
        match db::get_latest_log_date(pool, &sap_config.tenant_id).await? {
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

    let logs = api::fetch_sap_logs_paged(
        client,
        token,
        sap_config,
        opts.top,
        opts.page_size,
        filter.as_deref(),
        opts.log_concurrency,
        opts.fetch_error_details,
    )
    .await?;

    let count = logs.len();
    let queued =
        queue_exec_alerts_from_logs(pool, &sap_config.tenant_id, &logs, opts.alert_scan_limit)
            .await?;
    db::insert_logs(pool, &sap_config.tenant_id, logs).await?;
    Ok((count, queued))
}

async fn queue_exec_alerts_from_logs(
    pool: &sqlx::PgPool,
    tenant_id: &str,
    logs: &[LogEntry],
    limit: usize,
) -> Result<usize> {
    let mut queued = 0usize;
    for log in logs
        .iter()
        .filter(|l| l.status.as_deref() == Some("FAILED"))
        .take(limit)
    {
        let (Some(guid), Some(flow)) = (&log.message_guid, &log.integration_flow_name) else {
            continue;
        };
        let snippet = log
            .error_message
            .as_deref()
            .unwrap_or("")
            .chars()
            .take(200)
            .collect::<String>();
        db::insert_pending_alert(pool, tenant_id, guid, flow, "exec", &snippet).await?;
        queued += 1;
    }
    Ok(queued)
}

#[derive(Default)]
struct MetadataReport {
    packages: usize,
    artifacts: usize,
    deploy_errors: usize,
    configs: usize,
}

async fn sync_packages_artifacts_and_configs(
    pool: &sqlx::PgPool,
    client: &reqwest::Client,
    sap_config: &SapConfig,
    token: &str,
    opts: &SyncOptions,
) -> Result<MetadataReport> {
    let (packages_res, artifacts_res) = tokio::join!(
        api::fetch_packages(client, token, sap_config),
        api::fetch_artifacts(client, token, sap_config),
    );

    let packages = packages_res?;
    let artifacts = artifacts_res?;

    let pkg_ids: Vec<String> = packages.iter().filter_map(|p| p.id.clone()).collect();
    let error_ids: Vec<String> = artifacts
        .iter()
        .filter(|a| a.status.as_deref() == Some("ERROR"))
        .filter_map(|a| a.id.clone())
        .collect();

    let mut report = MetadataReport {
        packages: packages.len(),
        artifacts: artifacts.len(),
        ..Default::default()
    };

    let (pkg_insert, art_insert) = tokio::join!(
        db::insert_packages(pool, &sap_config.tenant_id, packages),
        db::insert_artifacts(pool, &sap_config.tenant_id, artifacts),
    );
    pkg_insert?;
    art_insert?;

    report.deploy_errors = sync_artifact_errors(
        pool,
        client,
        sap_config,
        token,
        error_ids,
        opts.metadata_concurrency,
    )
    .await?;

    if !opts.skip_configs && !pkg_ids.is_empty() {
        report.configs = sync_configurations(
            pool,
            client,
            sap_config,
            token,
            &pkg_ids,
            opts.metadata_concurrency,
        )
        .await?;
    }

    Ok(report)
}

async fn sync_artifact_errors(
    pool: &sqlx::PgPool,
    client: &reqwest::Client,
    sap_config: &SapConfig,
    token: &str,
    error_ids: Vec<String>,
    concurrency: usize,
) -> Result<usize> {
    let results: Vec<Result<usize>> = stream::iter(error_ids.into_iter())
        .map(|id| {
            let pool = pool.clone();
            let client = client.clone();
            let token = token.to_string();
            let sap_config = sap_config.clone();
            async move {
                match api::fetch_artifact_error(&client, &token, &sap_config, &id).await? {
                    Some(err_txt) => {
                        let snippet: String = err_txt.chars().take(200).collect();
                        db::insert_artifact_error(
                            &pool,
                            &sap_config.tenant_id,
                            ArtifactError {
                                artifact_id: id.clone(),
                                error_message: err_txt,
                                error_time: chrono::Utc::now().naive_utc(),
                            },
                        )
                        .await?;
                        db::insert_pending_alert(
                            &pool,
                            &sap_config.tenant_id,
                            &id,
                            &id,
                            "deploy",
                            &snippet,
                        )
                        .await?;
                        Ok(1)
                    }
                    None => Ok(0),
                }
            }
        })
        .buffer_unordered(concurrency.max(1))
        .collect()
        .await;

    let mut count = 0usize;
    for res in results {
        count += res?;
    }
    Ok(count)
}

async fn sync_configurations(
    pool: &sqlx::PgPool,
    client: &reqwest::Client,
    sap_config: &SapConfig,
    token: &str,
    pkg_ids: &[String],
    concurrency: usize,
) -> Result<usize> {
    let mapping =
        api::fetch_all_package_artifacts(client, token, sap_config, pkg_ids, concurrency).await;

    let pairs: Vec<(String, String)> = mapping
        .iter()
        .flat_map(|(pkg_id, art_ids)| art_ids.iter().map(move |aid| (pkg_id.clone(), aid.clone())))
        .collect();
    db::bulk_update_artifact_package(pool, &pairs).await?;

    let all_art_ids: Vec<String> = mapping.into_values().flatten().collect();
    let results: Vec<Result<usize>> = stream::iter(all_art_ids.into_iter())
        .map(|art_id| {
            let pool = pool.clone();
            let client = client.clone();
            let token = token.to_string();
            let sap_config = sap_config.clone();
            async move {
                let cfgs =
                    api::fetch_artifact_properties(&client, &token, &sap_config, &art_id).await?;
                let count = cfgs.len();
                db::insert_configurations(&pool, &sap_config.tenant_id, &art_id, cfgs).await?;
                Ok(count)
            }
        })
        .buffer_unordered(concurrency.max(1))
        .collect()
        .await;

    let mut count = 0usize;
    for res in results {
        count += res?;
    }
    Ok(count)
}

fn print_cycle_summary(summary: &CycleSummary) {
    let tenants_ok = summary.reports.len();
    let tenants_ko = summary.errors.len();
    let logs: usize = summary.reports.iter().map(|r| r.logs).sum();
    let packages: usize = summary.reports.iter().map(|r| r.packages).sum();
    let artifacts: usize = summary.reports.iter().map(|r| r.artifacts).sum();
    let configs: usize = summary.reports.iter().map(|r| r.configs).sum();
    let deploy_errors: usize = summary.reports.iter().map(|r| r.deploy_errors).sum();

    println!(
        "cycle terminé: ok={} ko={} logs={} packages={} artifacts={} configs={} deploy_errors={} elapsed={}ms",
        tenants_ok,
        tenants_ko,
        logs,
        packages,
        artifacts,
        configs,
        deploy_errors,
        summary.elapsed_ms
    );

    for err in &summary.errors {
        eprintln!("tenant error: {}", err);
    }
}
