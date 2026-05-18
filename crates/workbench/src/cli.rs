use std::{path::PathBuf, sync::OnceLock};

use anyhow::Result;
use clap::{ArgAction, Parser};
use serde::Deserialize;

const DEFAULT_PG_DSN: &str = "postgres://mmat:mmat@localhost:5432/mmat";
static PG_DSN: OnceLock<String> = OnceLock::new();
static LLM_CONFIG: OnceLock<Option<mmat_coordinator::WorkbenchAssistantConfig>> = OnceLock::new();

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

    /// LLM API key for runtime-backed assistant streaming
    #[arg(long = "llm-key", env = "MMAT_LLM_API_KEY")]
    llm_api_key: Option<String>,

    /// OpenAI-compatible LLM base URL
    #[arg(long = "llm-base-url", env = "MMAT_LLM_BASE_URL")]
    llm_base_url: Option<String>,

    /// LLM model for runtime-backed assistant streaming
    #[arg(long = "llm-model", env = "MMAT_LLM_MODEL")]
    llm_model: Option<String>,

    /// LLM request timeout in seconds
    #[arg(
        long = "llm-timeout-secs",
        env = "MMAT_LLM_TIMEOUT_SECS",
        default_value_t = 60
    )]
    llm_timeout_secs: u64,

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
    llm_api_key: Option<String>,
    llm_base_url: Option<String>,
    llm_model: Option<String>,
    llm_timeout_secs: Option<u64>,
    llm: Option<LlmConfigFile>,
    opencode_zen_api_key: Option<String>,
    project_root: Option<PathBuf>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
struct LlmConfigFile {
    api_key: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    timeout_secs: Option<u64>,
}

#[allow(dead_code)]
pub struct Config {
    pub bind_addr: String,
    pub pg_dsn: String,
    pub qdrant_url: String,
    pub qdrant_api_key: Option<String>,
    pub qdrant_collection: String,
    pub qdrant_vector_dimension: u64,
    pub llm_api_key: Option<String>,
    pub llm_base_url: Option<String>,
    pub llm_model: Option<String>,
    pub llm_timeout_secs: u64,
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

    let config = resolve_config(cli, cf)?;

    let _ = PG_DSN.set(config.pg_dsn.clone());
    let _ = LLM_CONFIG.set(assistant_config_from_values(
        config.llm_api_key.clone(),
        config.llm_base_url.clone(),
        config.llm_model.clone(),
        config.llm_timeout_secs,
    ));

    Ok(config)
}

fn resolve_config(cli: Cli, cf: Option<ConfigFile>) -> Result<Config> {
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
        llm_api_key: cf
            .as_ref()
            .and_then(config_file_llm_api_key)
            .or(cli.llm_api_key),
        llm_base_url: cf
            .as_ref()
            .and_then(config_file_llm_base_url)
            .or(cli.llm_base_url),
        llm_model: cf
            .as_ref()
            .and_then(config_file_llm_model)
            .or(cli.llm_model),
        llm_timeout_secs: cf
            .as_ref()
            .and_then(config_file_llm_timeout_secs)
            .unwrap_or(cli.llm_timeout_secs),
        opencode_zen_api_key,
        project_root,
        verbosity: cli.verbosity,
    };

    Ok(config)
}

fn config_file_llm_api_key(config: &ConfigFile) -> Option<String> {
    config
        .llm_api_key
        .clone()
        .or_else(|| config.llm.as_ref().and_then(|llm| llm.api_key.clone()))
}

fn config_file_llm_base_url(config: &ConfigFile) -> Option<String> {
    config
        .llm_base_url
        .clone()
        .or_else(|| config.llm.as_ref().and_then(|llm| llm.base_url.clone()))
}

fn config_file_llm_model(config: &ConfigFile) -> Option<String> {
    config
        .llm_model
        .clone()
        .or_else(|| config.llm.as_ref().and_then(|llm| llm.model.clone()))
}

fn config_file_llm_timeout_secs(config: &ConfigFile) -> Option<u64> {
    config
        .llm_timeout_secs
        .or_else(|| config.llm.as_ref().and_then(|llm| llm.timeout_secs))
}

pub fn pg_dsn() -> String {
    PG_DSN
        .get()
        .cloned()
        .or_else(|| std::env::var("MMAT_PG_DSN").ok())
        .unwrap_or_else(|| DEFAULT_PG_DSN.to_string())
}

pub fn llm_config() -> Option<mmat_coordinator::WorkbenchAssistantConfig> {
    LLM_CONFIG.get().cloned().flatten().or_else(|| {
        assistant_config_from_values(
            std::env::var("MMAT_LLM_API_KEY").ok(),
            std::env::var("MMAT_LLM_BASE_URL").ok(),
            std::env::var("MMAT_LLM_MODEL").ok(),
            std::env::var("MMAT_LLM_TIMEOUT_SECS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(60),
        )
    })
}

fn assistant_config_from_values(
    api_key: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    timeout_secs: u64,
) -> Option<mmat_coordinator::WorkbenchAssistantConfig> {
    Some(mmat_coordinator::WorkbenchAssistantConfig::new(
        api_key?,
        base_url,
        model?,
        std::time::Duration::from_secs(timeout_secs),
    ))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn cli() -> Cli {
        Cli {
            bind_addr: "127.0.0.1:8080".to_string(),
            pg_dsn: DEFAULT_PG_DSN.to_string(),
            qdrant_url: "http://localhost:6334".to_string(),
            qdrant_api_key: None,
            qdrant_collection: "mmat".to_string(),
            qdrant_vector_dimensions: 64,
            llm_api_key: None,
            llm_base_url: None,
            llm_model: None,
            llm_timeout_secs: 60,
            opencode_zen_api_key: None,
            project_root: None,
            verbosity: 0,
            config: Some(PathBuf::from("mmat.toml")),
        }
    }

    #[test]
    fn resolves_nested_llm_config_table() {
        let config_file = toml::from_str::<ConfigFile>(
            r#"
opencode_zen_api_key = "sk-opencode"
project_root = "/tmp/projects"

[llm]
api_key = "sk-llm"
base_url = "https://llm.example/v1"
model = "test-model"
timeout_secs = 45
"#,
        )
        .unwrap();

        let config = resolve_config(cli(), Some(config_file)).unwrap();

        assert_eq!(config.llm_api_key.as_deref(), Some("sk-llm"));
        assert_eq!(
            config.llm_base_url.as_deref(),
            Some("https://llm.example/v1")
        );
        assert_eq!(config.llm_model.as_deref(), Some("test-model"));
        assert_eq!(config.llm_timeout_secs, 45);
    }

    #[test]
    fn resolves_flat_llm_config_keys() {
        let config_file = toml::from_str::<ConfigFile>(
            r#"
opencode_zen_api_key = "sk-opencode"
project_root = "/tmp/projects"
llm_api_key = "sk-llm"
llm_base_url = "https://llm.example/v1"
llm_model = "test-model"
llm_timeout_secs = 45
"#,
        )
        .unwrap();

        let config = resolve_config(cli(), Some(config_file)).unwrap();

        assert_eq!(config.llm_api_key.as_deref(), Some("sk-llm"));
        assert_eq!(
            config.llm_base_url.as_deref(),
            Some("https://llm.example/v1")
        );
        assert_eq!(config.llm_model.as_deref(), Some("test-model"));
        assert_eq!(config.llm_timeout_secs, 45);
    }
}
