use clap::Parser;
use serde::Deserialize;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use mmat_memory::librarian::Librarian;
use mmat_memory::qdrant::{QdrantMemoryBackend, QdrantMemoryConfig};
use mmat_memory::vector_backend::{NoopVectorBackend, VectorMemoryBackend};
use mmat_workbench::{
    DEFAULT_BIND_ADDR, WorkbenchError, build_app_router, build_runtime, seed_workbench,
    spawn_projection_task, startup_projection,
};
use tokio::signal;
use tracing::{error, info};

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Path to configuration file (mmat.toml)
    #[arg(short = 'c', long = "config", env = "MMAT_CONFIG")]
    config: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
struct Config {
    db_url: Option<String>,
    workbench_addr: Option<String>,
    qdrant_url: Option<String>,
    qdrant_api_key: Option<String>,
    qdrant_collection: Option<String>,
    qdrant_vector_dimension: Option<u64>,
    opencode_zen_api_key: Option<String>,
    project_dir: Option<String>,
    rust_log: Option<String>,
}

fn set_env_if_unset(key: &str, value: &str) {
    if std::env::var(key).is_err() {
        // Safety: called early in main() before any threads are spawned
        unsafe {
            std::env::set_var(key, value);
        }
    }
}

fn apply_config(config: &Config) {
    if let Some(ref v) = config.db_url {
        set_env_if_unset("MMAT_DB_URL", v);
    }
    if let Some(ref v) = config.workbench_addr {
        set_env_if_unset("MMAT_WORKBENCH_ADDR", v);
    }
    if let Some(ref v) = config.qdrant_url {
        set_env_if_unset("MMAT_QDRANT_URL", v);
    }
    if let Some(ref v) = config.qdrant_api_key {
        set_env_if_unset("MMAT_QDRANT_API_KEY", v);
    }
    if let Some(ref v) = config.qdrant_collection {
        set_env_if_unset("MMAT_QDRANT_COLLECTION", v);
    }
    if let Some(v) = config.qdrant_vector_dimension {
        set_env_if_unset("MMAT_QDRANT_VECTOR_DIMENSION", &v.to_string());
    }
    if let Some(ref v) = config.opencode_zen_api_key {
        set_env_if_unset("MMAT_OPENCODE_ZEN_API_KEY", v);
    }
    if let Some(ref v) = config.project_dir {
        set_env_if_unset("MMAT_PROJECT_DIR", v);
    }
    if let Some(ref v) = config.rust_log {
        set_env_if_unset("RUST_LOG", v);
    }
}

fn load_config(path: &std::path::Path) -> Result<Config, WorkbenchError> {
    let contents = std::fs::read_to_string(path).map_err(|e| {
        WorkbenchError::Init(format!(
            "failed to read config file {}: {}",
            path.display(),
            e
        ))
    })?;
    toml::from_str::<Config>(&contents).map_err(|e| {
        WorkbenchError::Init(format!(
            "failed to parse config file {}: {}",
            path.display(),
            e
        ))
    })
}

#[tokio::main]
async fn main() -> Result<(), WorkbenchError> {
    let cli = Cli::parse();

    if let Some(ref config_path) = cli.config {
        let config = load_config(config_path)?;
        apply_config(&config);
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "mmat_workbench=info".to_string()),
        )
        .init();

    let bind_addr =
        std::env::var("MMAT_WORKBENCH_ADDR").unwrap_or_else(|_| DEFAULT_BIND_ADDR.to_string());
    let socket_addr =
        bind_addr
            .parse::<SocketAddr>()
            .map_err(|source| WorkbenchError::InvalidBindAddress {
                address: bind_addr.clone(),
                source,
            })?;

    let (state, runtime) = build_runtime().await?;
    spawn_projection_task(state.clone());
    startup_projection(&state).await;

    let librarian_bus: Arc<_> = runtime.bus().clone().into();
    let librarian_store = runtime.memory_store().clone();
    let vector_backend = build_vector_backend().await?;
    let librarian = Librarian::new(librarian_store, vector_backend, Duration::from_secs(3600));
    tokio::spawn(async move {
        if let Err(err) = librarian.run(librarian_bus).await {
            error!("Librarian stopped with error: {}", err);
        }
    });

    tokio::spawn(async move {
        if let Err(err) = runtime.run().await {
            error!("MMAT organisation runtime stopped with error: {}", err);
        }
    });
    seed_workbench(&state).await;

    let app = build_app_router(state);

    info!("static assets compiled into binary (index.html, style.css, app.js)");

    let listener = tokio::net::TcpListener::bind(socket_addr)
        .await
        .map_err(|source| WorkbenchError::Bind {
            address: socket_addr.to_string(),
            source,
        })?;
    info!("MMAT workbench listening on http://{}", socket_addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(WorkbenchError::Server)
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("shutdown signal received, starting graceful shutdown");
}

async fn build_vector_backend() -> Result<Arc<dyn VectorMemoryBackend>, WorkbenchError> {
    let Ok(url) = std::env::var("MMAT_QDRANT_URL") else {
        return Ok(Arc::new(NoopVectorBackend));
    };

    let vector_dimension = std::env::var("MMAT_QDRANT_VECTOR_DIMENSION")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(64);
    let config = QdrantMemoryConfig {
        url,
        api_key: std::env::var("MMAT_QDRANT_API_KEY").ok(),
        collection_name: std::env::var("MMAT_QDRANT_COLLECTION")
            .unwrap_or_else(|_| "memories".to_string()),
        vector_dimension,
    };

    let backend = QdrantMemoryBackend::new(config)
        .await
        .map_err(|err| WorkbenchError::Init(format!("failed to initialise Qdrant: {err}")))?;
    Ok(Arc::new(backend))
}
