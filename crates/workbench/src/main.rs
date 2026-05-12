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

#[tokio::main]
async fn main() -> Result<(), WorkbenchError> {
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
