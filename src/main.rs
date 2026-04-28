mod api;
mod config;
mod crypto;
mod db;
mod models;
mod queries;
mod tenant_setup;
mod webhook;

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use config::{SapConfig, TokenCache};
use futures::stream::{self, StreamExt};
use models::{ArtifactError, LogEntry};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum BackfillStrategy {
    /// Pagination classique avec $skip. Tres rapide sur des volumes moyens, mais ralentit quand $skip devient profond.
    Offset,
    /// Pagination par curseur temporel: LogStart < derniere date recue. Recommande pour les gros backfills.
    Cursor,
}

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

    /// Taille logique demandée pour une page OData MessageProcessingLogs.
    /// SAP CPI plafonne souvent silencieusement à 1000 résultats par page.
    #[arg(long, default_value_t = 1_000)]
    page_size: u32,

    /// Nombre de pages MessageProcessingLogs récupérées en parallèle en mode backfill full.
    /// Garder 1 pour le mode le plus sûr. Monter à 3-6 pour les backfills rapides.
    #[arg(long, default_value_t = 1)]
    page_concurrency: usize,

    /// Strategie de backfill full pour les logs.
    /// offset = $skip classique, rapide sur petits volumes.
    /// cursor = filtre LogStart < derniere_date_vue, plus stable sur gros volumes.
    #[arg(long, value_enum, default_value = "offset")]
    backfill_strategy: BackfillStrategy,

    /// Nombre de logs gardés en mémoire avant un COPY PostgreSQL.
    /// Plus grand = moins de merges DB, mais plus de RAM utilisée.
    #[arg(long, default_value_t = 20_000)]
    insert_buffer_rows: usize,

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

    /// Ne calcule ni pending_alerts ni smart alerts pendant ce cycle.
    /// Recommandé pour les backfills massifs et les benchmarks d ingestion.
    #[arg(long)]
    skip_alerts: bool,

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
    page_concurrency: usize,
    backfill_strategy: BackfillStrategy,
    insert_buffer_rows: usize,
    full: bool,
    log_concurrency: usize,
    metadata_concurrency: usize,
    tenant_concurrency: usize,
    logs_only: bool,
    metadata_only: bool,
    skip_configs: bool,
    fetch_error_details: bool,
    queue_alerts: bool,
    alert_scan_limit: usize,
}

impl From<&Cli> for SyncOptions {
    fn from(cli: &Cli) -> Self {
        let top = cli.top.max(1);
        Self {
            top,
            page_size: cli.page_size.clamp(1, 5_000).min(top),
            page_concurrency: cli.page_concurrency.max(1).min(16),
            backfill_strategy: cli.backfill_strategy,
            insert_buffer_rows: cli.insert_buffer_rows.clamp(1_000, 200_000),
            full: cli.full,
            log_concurrency: cli.log_concurrency.max(1),
            metadata_concurrency: cli.metadata_concurrency.max(1),
            tenant_concurrency: cli.tenant_concurrency,
            logs_only: cli.logs_only,
            metadata_only: cli.metadata_only,
            skip_configs: cli.skip_configs,
            fetch_error_details: !cli.skip_error_details,
            queue_alerts: !cli.skip_alerts && cli.alert_scan_limit > 0,
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

    if !incremental && filter.is_none() {
        match opts.backfill_strategy {
            BackfillStrategy::Cursor => {
                sync_logs_cursor_backfill(pool, client, sap_config, token, opts).await
            }
            BackfillStrategy::Offset if opts.page_concurrency > 1 => {
                sync_logs_parallel_backfill(pool, client, sap_config, token, opts).await
            }
            BackfillStrategy::Offset => {
                sync_logs_sequential_buffered(pool, client, sap_config, token, opts, None).await
            }
        }
    } else {
        // En incremental, on garde le chemin sequentiel avec filtre LogStart gt dernier log connu.
        sync_logs_sequential_buffered(pool, client, sap_config, token, opts, filter.as_deref())
            .await
    }
}

async fn sync_logs_sequential_buffered(
    pool: &sqlx::PgPool,
    client: &reqwest::Client,
    sap_config: &SapConfig,
    token: &str,
    opts: &SyncOptions,
    filter: Option<&str>,
) -> Result<(usize, usize)> {
    let mut total_logs = 0usize;
    let mut total_alerts = 0usize;
    let mut skip = 0u32;
    let mut buffer: Vec<LogEntry> =
        Vec::with_capacity(opts.insert_buffer_rows.min(opts.top as usize));

    while skip < opts.top {
        let wanted = (opts.top - skip).min(opts.page_size);
        let logs = api::fetch_sap_logs_page(
            client,
            token,
            sap_config,
            wanted,
            skip,
            filter,
            opts.log_concurrency,
            opts.fetch_error_details,
        )
        .await?;

        let fetched = logs.len();
        if fetched == 0 {
            break;
        }

        if opts.queue_alerts && total_alerts < opts.alert_scan_limit {
            total_alerts += queue_exec_alerts_from_logs(
                pool,
                &sap_config.tenant_id,
                &logs,
                opts.alert_scan_limit.saturating_sub(total_alerts),
            )
            .await?;
        }

        buffer.extend(logs);
        if buffer.len() >= opts.insert_buffer_rows {
            db::insert_logs(pool, &sap_config.tenant_id, std::mem::take(&mut buffer)).await?;
        }

        total_logs += fetched;

        log::info!(
            "[{}] logs page OK: fetched={} total={} skip_next={}",
            sap_config.tenant_id,
            fetched,
            total_logs,
            skip.saturating_add(fetched as u32)
        );

        // SAP CPI peut limiter une page à 1000 lignes même si on demande 5000.
        // Ne pas stopper sur `fetched < wanted`, sinon `--top 50000 --page-size 5000`
        // s'arrête à 1000. On continue avec `$skip += fetched` jusqu'à `top` ou page vide.
        skip = skip.saturating_add(fetched as u32);
    }

    if !buffer.is_empty() {
        db::insert_logs(pool, &sap_config.tenant_id, buffer).await?;
    }

    Ok((total_logs, total_alerts))
}

async fn sync_logs_cursor_backfill(
    pool: &sqlx::PgPool,
    client: &reqwest::Client,
    sap_config: &SapConfig,
    token: &str,
    opts: &SyncOptions,
) -> Result<(usize, usize)> {
    // Cette strategie evite les `$skip` profonds.
    // Au lieu de demander `skip=250000`, elle demande toujours la premiere page
    // des logs plus anciens que le plus vieux LogStart deja recu.
    let effective_page_size = opts.page_size.min(1_000).max(1);
    let mut total_logs = 0usize;
    let mut total_alerts = 0usize;
    let mut cursor: Option<chrono::NaiveDateTime> = None;
    let mut buffer: Vec<LogEntry> =
        Vec::with_capacity(opts.insert_buffer_rows.min(opts.top as usize));

    log::info!(
        "[{}] backfill cursor: page_size={} insert_buffer_rows={} top={}",
        sap_config.tenant_id,
        effective_page_size,
        opts.insert_buffer_rows,
        opts.top
    );

    while total_logs < opts.top as usize {
        let remaining = (opts.top as usize).saturating_sub(total_logs);
        let wanted = effective_page_size.min(remaining as u32);
        let filter =
            cursor.map(|dt| format!("LogStart lt datetime'{}'", format_odata_datetime(dt)));

        let logs = api::fetch_sap_logs_page(
            client,
            token,
            sap_config,
            wanted,
            0,
            filter.as_deref(),
            opts.log_concurrency,
            opts.fetch_error_details,
        )
        .await?;

        let fetched = logs.len();
        if fetched == 0 {
            break;
        }

        let oldest = logs.iter().filter_map(|log| log.parsed_date).min();

        if opts.queue_alerts && total_alerts < opts.alert_scan_limit {
            total_alerts += queue_exec_alerts_from_logs(
                pool,
                &sap_config.tenant_id,
                &logs,
                opts.alert_scan_limit.saturating_sub(total_alerts),
            )
            .await?;
        }

        buffer.extend(logs);
        total_logs += fetched;

        log::info!(
            "[{}] logs cursor page OK: fetched={} total={} next_before={}",
            sap_config.tenant_id,
            fetched,
            total_logs,
            oldest
                .map(format_odata_datetime)
                .unwrap_or_else(|| "n/a".to_string())
        );

        if buffer.len() >= opts.insert_buffer_rows {
            db::insert_logs(pool, &sap_config.tenant_id, std::mem::take(&mut buffer)).await?;
        }

        let Some(oldest) = oldest else {
            log::warn!(
                "[{}] backfill cursor stoppe: aucune parsed_date dans la derniere page",
                sap_config.tenant_id
            );
            break;
        };

        if cursor == Some(oldest) {
            log::warn!(
                "[{}] backfill cursor stoppe: curseur bloque sur {}",
                sap_config.tenant_id,
                format_odata_datetime(oldest)
            );
            break;
        }

        cursor = Some(oldest);

        if fetched < wanted as usize {
            break;
        }
    }

    if !buffer.is_empty() {
        db::insert_logs(pool, &sap_config.tenant_id, buffer).await?;
    }

    Ok((total_logs, total_alerts))
}

fn format_odata_datetime(dt: chrono::NaiveDateTime) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S").to_string()
}

async fn sync_logs_parallel_backfill(
    pool: &sqlx::PgPool,
    client: &reqwest::Client,
    sap_config: &SapConfig,
    token: &str,
    opts: &SyncOptions,
) -> Result<(usize, usize)> {
    // Important: SAP CPI renvoie souvent 1000 lignes maximum même si `$top=5000`.
    // En parallèle, `$skip` est pré-calculé: il faut donc utiliser un stride fiable.
    let sap_page_stride = opts.page_size.min(1_000).max(1);
    let mut next_skip = 0u32;
    let mut total_logs = 0usize;
    let mut total_alerts = 0usize;
    let mut stop = false;
    let mut buffer: Vec<LogEntry> =
        Vec::with_capacity(opts.insert_buffer_rows.min(opts.top as usize));

    log::info!(
        "[{}] backfill parallèle: page_concurrency={} sap_page_stride={} insert_buffer_rows={}",
        sap_config.tenant_id,
        opts.page_concurrency,
        sap_page_stride,
        opts.insert_buffer_rows
    );

    while next_skip < opts.top && !stop {
        let mut offsets = Vec::with_capacity(opts.page_concurrency);
        for _ in 0..opts.page_concurrency {
            if next_skip >= opts.top {
                break;
            }
            let wanted = (opts.top - next_skip).min(sap_page_stride);
            offsets.push((next_skip, wanted));
            next_skip = next_skip.saturating_add(wanted);
        }

        let mut pages: Vec<Result<(u32, u32, Vec<LogEntry>)>> = stream::iter(offsets.into_iter())
            .map(|(skip, wanted)| {
                let client = client.clone();
                let token = token.to_string();
                let cfg = sap_config.clone();
                let opts = opts.clone();
                async move {
                    let logs = api::fetch_sap_logs_page(
                        &client,
                        &token,
                        &cfg,
                        wanted,
                        skip,
                        None,
                        opts.log_concurrency,
                        opts.fetch_error_details,
                    )
                    .await?;
                    Ok((skip, wanted, logs))
                }
            })
            .buffer_unordered(opts.page_concurrency)
            .collect()
            .await;

        pages.sort_by_key(|res| match res {
            Ok((skip, _, _)) => *skip,
            Err(_) => u32::MAX,
        });

        for page in pages {
            let (skip, wanted, logs) = page?;
            let fetched = logs.len();

            if fetched == 0 {
                stop = true;
                continue;
            }

            if opts.queue_alerts && total_alerts < opts.alert_scan_limit {
                total_alerts += queue_exec_alerts_from_logs(
                    pool,
                    &sap_config.tenant_id,
                    &logs,
                    opts.alert_scan_limit.saturating_sub(total_alerts),
                )
                .await?;
            }

            buffer.extend(logs);
            total_logs += fetched;

            log::info!(
                "[{}] logs page OK: fetched={} total={} skip_next={}",
                sap_config.tenant_id,
                fetched,
                total_logs,
                skip.saturating_add(fetched as u32)
            );

            if buffer.len() >= opts.insert_buffer_rows {
                db::insert_logs(pool, &sap_config.tenant_id, std::mem::take(&mut buffer)).await?;
            }

            // Ici `wanted` vaut au plus `sap_page_stride`, donc `fetched < wanted` est un vrai signal
            // de fin de collection. Ce n'est pas le cas en séquentiel avec page_size=5000.
            if fetched < wanted as usize || total_logs >= opts.top as usize {
                stop = true;
            }
        }
    }

    if !buffer.is_empty() {
        db::insert_logs(pool, &sap_config.tenant_id, buffer).await?;
    }

    Ok((total_logs, total_alerts))
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
        opts.queue_alerts,
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
    queue_alerts: bool,
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
                        if queue_alerts {
                            db::insert_pending_alert(
                                &pool,
                                &sap_config.tenant_id,
                                &id,
                                &id,
                                "deploy",
                                &snippet,
                            )
                            .await?;
                        }
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
