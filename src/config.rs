use anyhow::{Context, Result};
use clap::Parser;

use crate::models::CliConfig;

#[derive(Parser, Debug)]
#[command(author, version, about = "Read-only SAP BTP PostgreSQL TUI")]
pub struct Cli {
    #[arg(long)]
    pub tenant: Option<String>,

    #[arg(long, default_value_t = 500)]
    pub limit: i64,

    #[arg(long, default_value_t = 10)]
    pub refresh_seconds: u64,
}

pub struct Config {
    pub database_url: String,
    pub cli: CliConfig,
}

impl Config {
    pub fn load() -> Result<Self> {
        dotenvy::dotenv().ok();
        let cli = Cli::parse();
        let database_url =
            std::env::var("DATABASE_URL").context("DATABASE_URL doit etre defini")?;

        Ok(Self {
            database_url,
            cli: CliConfig {
                tenant: cli.tenant,
                limit: cli.limit.max(1),
                refresh_seconds: cli.refresh_seconds.max(1),
            },
        })
    }
}
