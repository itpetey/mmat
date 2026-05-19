use std::{path::PathBuf, sync::OnceLock};

use anyhow::Result;
use clap::{ArgAction, Parser};
use serde::Deserialize;

const DEFAULT_PG_DSN: &str = "postgres://mmat:mmat@localhost:5432/mmat";
static LLM_CONFIG: OnceLock<Option<mmat_coordinator::WorkbenchAssistantConfig>> = OnceLock::new();
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

    /// LLM API key for role-level LLM and workbench assistant streaming
    #[arg(long = "llm-key", env = "MMAT_LLM_API_KEY")]
    llm_api_key: Option<String>,

    /// LLM model identifier
    #[arg(long = "llm-model", env = "MMAT_LLM_MODEL")]
    llm_model: Option<String>,

    /// LLM request timeout in seconds
    #[arg(
        long = "llm-timeout-secs",
        env = "MMAT_LLM_TIMEOUT_SECS",
        default_value_t = 60
    )]
    llm_timeout_secs: u64,

    /// Directory to store project repositories
    #[arg(long = "proj", env = "MMAT_PROJECT_ROOT")]
    project_root: Option<PathBuf>,

    /// Increase log verbosity (repeat for more: -v, -vv, -vvv)
    #[arg(short = 'v', long = "verbose", action = ArgAction::Count)]
    verbosity: u8,

    /// Path to configuration file
    #[arg(short = 'c', long = "config", env = "MMAT_CONFIG")]
    config: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
struct MmatSection {
    bind_addr: Option<String>,
    project_root: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
struct PostgresSection {
    dsn: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct QdrantSection {
    url: Option<String>,
    api_key: Option<String>,
    collection: Option<String>,
    vector_dimension: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
struct LlmSection {
    api_key: Option<String>,
    model: Option<String>,
    timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConfigFile {
    mmat: Option<MmatSection>,
    postgres: Option<PostgresSection>,
    qdrant: Option<QdrantSection>,
    llm: Option<LlmSection>,
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct MmatConfig {
    pub bind_addr: String,
    pub project_root: PathBuf,
}

#[derive(Debug)]
pub struct PostgresConfig {
    pub dsn: String,
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct QdrantConfig {
    pub url: String,
    pub api_key: Option<String>,
    pub collection: String,
    pub vector_dimension: u64,
}

#[derive(Debug)]
pub struct LlmConfig {
    pub api_key: String,
    pub model: String,
    pub timeout_secs: u64,
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct Config {
    pub mmat: MmatConfig,
    pub postgres: PostgresConfig,
    pub qdrant: QdrantConfig,
    pub llm: LlmConfig,
    pub verbosity: u8,
}

pub fn llm_config() -> Option<mmat_coordinator::WorkbenchAssistantConfig> {
    LLM_CONFIG.get().cloned().flatten().or_else(|| {
        Some(mmat_coordinator::WorkbenchAssistantConfig::new(
            std::env::var("MMAT_LLM_API_KEY").ok()?,
            std::env::var("MMAT_LLM_MODEL").ok()?,
            std::time::Duration::from_secs(
                std::env::var("MMAT_LLM_TIMEOUT_SECS")
                    .ok()
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(60),
            ),
        ))
    })
}

#[allow(dead_code)]
pub fn load_config() -> Result<Config> {
    let cli = Cli::parse();

    let cf: Option<ConfigFile> = match &cli.config {
        Some(path) => Some(toml::from_str(&std::fs::read_to_string(path)?)?),
        None => None,
    };

    let config = resolve_config(cli, cf)?;

    let _ = PG_DSN.set(config.postgres.dsn.clone());
    let _ = LLM_CONFIG.set(Some(mmat_coordinator::WorkbenchAssistantConfig::new(
        config.llm.api_key.clone(),
        config.llm.model.clone(),
        std::time::Duration::from_secs(config.llm.timeout_secs),
    )));

    Ok(config)
}

pub fn pg_dsn() -> String {
    PG_DSN
        .get()
        .cloned()
        .or_else(|| std::env::var("MMAT_PG_DSN").ok())
        .unwrap_or_else(|| DEFAULT_PG_DSN.to_string())
}

fn resolve_config(cli: Cli, cf: Option<ConfigFile>) -> Result<Config> {
    let mmat = {
        let s = cf.as_ref().and_then(|c| c.mmat.as_ref());
        let bind_addr = s.and_then(|s| s.bind_addr.clone()).unwrap_or(cli.bind_addr);
        let project_root = cli
            .project_root
            .or_else(|| s.and_then(|s| s.project_root.clone()))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "mmat.project_root must be provided via --proj/MMAT_PROJECT_ROOT or [mmat] section in config file"
                )
            })?;
        MmatConfig {
            bind_addr,
            project_root,
        }
    };

    let postgres = {
        let dsn = cf
            .as_ref()
            .and_then(|c| c.postgres.as_ref())
            .and_then(|s| s.dsn.clone())
            .unwrap_or(cli.pg_dsn);
        PostgresConfig { dsn }
    };

    let qdrant = {
        let s = cf.as_ref().and_then(|c| c.qdrant.as_ref());
        QdrantConfig {
            url: s.and_then(|s| s.url.clone()).unwrap_or(cli.qdrant_url),
            api_key: s.and_then(|s| s.api_key.clone()).or(cli.qdrant_api_key),
            collection: s
                .and_then(|s| s.collection.clone())
                .unwrap_or(cli.qdrant_collection),
            vector_dimension: s
                .and_then(|s| s.vector_dimension)
                .unwrap_or(cli.qdrant_vector_dimensions),
        }
    };

    let llm = {
        let s = cf.as_ref().and_then(|c| c.llm.as_ref());
        let api_key = cli
            .llm_api_key
            .or_else(|| s.and_then(|s| s.api_key.clone()))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "llm.api_key must be provided via --llm-key/MMAT_LLM_API_KEY or [llm] section in config file"
                )
            })?;
        let model = cli
            .llm_model
            .or_else(|| s.and_then(|s| s.model.clone()))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "llm.model must be provided via --llm-model/MMAT_LLM_MODEL or [llm] section in config file"
                )
            })?;
        LlmConfig {
            api_key,
            model,
            timeout_secs: s
                .and_then(|s| s.timeout_secs)
                .unwrap_or(cli.llm_timeout_secs),
        }
    };

    Ok(Config {
        mmat,
        postgres,
        qdrant,
        llm,
        verbosity: cli.verbosity,
    })
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
            llm_model: None,
            llm_timeout_secs: 60,
            project_root: None,
            verbosity: 0,
            config: Some(PathBuf::from("mmat.toml")),
        }
    }

    #[test]
    fn errors_on_missing_required_fields() {
        // project_root is checked first, so that is the error we see.
        let result = resolve_config(cli(), None);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("mmat.project_root")
        );

        // When project_root is provided but LLM is missing, error shifts to LLM.
        let mut with_proj = cli();
        with_proj.project_root = Some(PathBuf::from("/tmp/projects"));
        let result = resolve_config(with_proj, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("llm.api_key"));
    }

    #[test]
    fn full_hierarchical_config() {
        let config_file = toml::from_str::<ConfigFile>(
            r#"
[mmat]
bind_addr = "0.0.0.0:9090"
project_root = "/data/projects"

[postgres]
dsn = "postgres://user:pass@host/db"

[qdrant]
url = "https://qdrant.example:6334"
api_key = "qd-secret"
collection = "my-collection"
vector_dimension = 128

[llm]
api_key = "sk-llm"
model = "gpt-4o"
timeout_secs = 120
"#,
        )
        .unwrap();

        let cfg = resolve_config(cli(), Some(config_file)).unwrap();

        assert_eq!(cfg.mmat.bind_addr, "0.0.0.0:9090");
        assert_eq!(cfg.mmat.project_root, PathBuf::from("/data/projects"));
        assert_eq!(cfg.postgres.dsn, "postgres://user:pass@host/db");
        assert_eq!(cfg.qdrant.url, "https://qdrant.example:6334");
        assert_eq!(cfg.qdrant.api_key.as_deref(), Some("qd-secret"));
        assert_eq!(cfg.qdrant.collection, "my-collection");
        assert_eq!(cfg.qdrant.vector_dimension, 128);
        assert_eq!(cfg.llm.api_key, "sk-llm");
        assert_eq!(cfg.llm.model, "gpt-4o");
        assert_eq!(cfg.llm.timeout_secs, 120);
    }

    #[test]
    fn cli_llm_values_override_config_file() {
        let mut my_cli = cli();
        my_cli.project_root = Some(PathBuf::from("/cli/projects"));
        my_cli.llm_api_key = Some("cli-key".to_string());
        my_cli.llm_model = Some("cli-model".to_string());

        let config_file = toml::from_str::<ConfigFile>(
            r#"
[mmat]
project_root = "/file/projects"

[llm]
api_key = "file-key"
model = "file-model"
"#,
        )
        .unwrap();

        let cfg = resolve_config(my_cli, Some(config_file)).unwrap();
        assert_eq!(cfg.mmat.project_root, PathBuf::from("/cli/projects"));
        assert_eq!(cfg.llm.api_key, "cli-key");
        assert_eq!(cfg.llm.model, "cli-model");
    }

    #[test]
    fn minimal_config_provides_defaults() {
        let config_file = toml::from_str::<ConfigFile>(
            r#"
[mmat]
project_root = "/tmp/projects"

[llm]
api_key = "sk-test"
model = "test-model"
"#,
        )
        .unwrap();

        let cfg = resolve_config(cli(), Some(config_file)).unwrap();
        assert_eq!(cfg.mmat.bind_addr, "127.0.0.1:8080");
        assert_eq!(cfg.postgres.dsn, DEFAULT_PG_DSN);
        assert_eq!(cfg.qdrant.url, "http://localhost:6334");
        assert_eq!(cfg.qdrant.collection, "mmat");
        assert_eq!(cfg.qdrant.vector_dimension, 64);
        assert_eq!(cfg.llm.timeout_secs, 60);
    }
}
