use std::{path::PathBuf, sync::OnceLock};

use anyhow::Result;
use clap::{ArgAction, Parser};
use serde::Deserialize;

const DEFAULT_PG_DSN: &str = "postgres://mmat:mmat@localhost:5432/mmat";
static PG_DSN: OnceLock<String> = OnceLock::new();

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Address to bind the web server to
    #[arg(
        short = 'a',
        long = "addr",
        env = "MMAT_ADDR",
        default_value = "127.0.0.1:8080"
    )]
    bind_addr: String,

    /// PostgreSQL data source name
    #[arg(
        long = "pg",
        env = "MMAT_PG_DSN",
        default_value = DEFAULT_PG_DSN
    )]
    pg_dsn: String,

    /// Qdrant connection URL
    #[arg(
        long = "qd",
        env = "MMAT_QD_URL",
        default_value = "http://localhost:6334"
    )]
    qdrant_url: String,

    /// Qdrant API key (if needed)
    #[arg(long = "qd-key", env = "MMAT_QD_KEY")]
    qdrant_api_key: Option<String>,

    /// Qdrant collection name
    #[arg(
        long = "qd-collection",
        env = "MMAT_QD_COLLECTION",
        default_value = "mmat"
    )]
    qdrant_collection: String,

    /// Number of Qdrant vector dimensions
    #[arg(
        long = "qd-dimensions",
        env = "MMAT_QD_DIMENSIONS",
        default_value_t = 64
    )]
    qdrant_vector_dimensions: u64,

    /// OpenCode API key
    #[arg(
        long = "key",
        env = "MMAT_OPENCODE_KEY",
        required_unless_present = "config",
        requires_all = ["project_root"],
    )]
    opencode_zen_api_key: Option<String>,

    /// Directory to store project repositories
    #[arg(
        long = "proj",
        env = "MMAT_PROJECT_ROOT",
        required_unless_present = "config",
        requires_all = ["opencode_zen_api_key"],
    )]
    project_root: Option<PathBuf>,

    /// Increase log verbosity (repeat for more: -v, -vv, -vvv)
    #[arg(short = 'v', long = "verbose", action = ArgAction::Count)]
    verbosity: u8,

    /// Path to configuration file
    #[arg(short = 'c', long = "config", env = "MMAT_CONFIG")]
    config: Option<PathBuf>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
struct ConfigFile {
    bind_addr: Option<String>,
    pg_dsn: Option<String>,
    qdrant_url: Option<String>,
    qdrant_api_key: Option<String>,
    qdrant_collection: Option<String>,
    qdrant_vector_dimension: Option<u64>,
    opencode_zen_api_key: Option<String>,
    project_root: Option<PathBuf>,
}

#[allow(dead_code)]
pub struct Config {
    pub bind_addr: String,
    pub pg_dsn: String,
    pub qdrant_url: String,
    pub qdrant_api_key: Option<String>,
    pub qdrant_collection: String,
    pub qdrant_vector_dimension: u64,
    pub opencode_zen_api_key: String,
    pub project_root: PathBuf,
    pub verbosity: u8,
}

#[allow(dead_code)]
pub fn load_config() -> Result<Config> {
    let cli = Cli::parse();

    let cf = match &cli.config {
        Some(path) => Some(toml::from_str::<ConfigFile>(&std::fs::read_to_string(
            path,
        )?)?),
        None => None,
    };

    let opencode_zen_api_key = cli
        .opencode_zen_api_key
        .or_else(|| cf.as_ref().and_then(|c| c.opencode_zen_api_key.clone()))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "opencode_zen_api_key must be provided via --key/MMAT_OPENCODE_KEY or config file"
            )
        })?;

    let project_root = cli
        .project_root
        .or_else(|| cf.as_ref().and_then(|c| c.project_root.clone()))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "project_root must be provided via --proj/MMAT_PROJECT_ROOT or config file"
            )
        })?;

    let config = Config {
        bind_addr: cf
            .as_ref()
            .and_then(|c| c.bind_addr.clone())
            .unwrap_or(cli.bind_addr),
        pg_dsn: cf
            .as_ref()
            .and_then(|c| c.pg_dsn.clone())
            .unwrap_or(cli.pg_dsn),
        qdrant_url: cf
            .as_ref()
            .and_then(|c| c.qdrant_url.clone())
            .unwrap_or(cli.qdrant_url),
        qdrant_api_key: cf
            .as_ref()
            .and_then(|c| c.qdrant_api_key.clone())
            .or(cli.qdrant_api_key),
        qdrant_collection: cf
            .as_ref()
            .and_then(|c| c.qdrant_collection.clone())
            .unwrap_or(cli.qdrant_collection),
        qdrant_vector_dimension: cf
            .as_ref()
            .and_then(|c| c.qdrant_vector_dimension)
            .unwrap_or(cli.qdrant_vector_dimensions),
        opencode_zen_api_key,
        project_root,
        verbosity: cli.verbosity,
    };

    let _ = PG_DSN.set(config.pg_dsn.clone());

    Ok(config)
}

pub fn pg_dsn() -> String {
    PG_DSN
        .get()
        .cloned()
        .or_else(|| std::env::var("MMAT_PG_DSN").ok())
        .unwrap_or_else(|| DEFAULT_PG_DSN.to_string())
}
