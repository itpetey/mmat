use std::net::SocketAddr;

use mmat_workbench::{
    DEFAULT_BIND_ADDR, WorkbenchError, build_app_router, build_runtime, seed_workbench,
    spawn_projection_task,
};
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

    let (state, runtime) = build_runtime()?;
    spawn_projection_task(state.clone());
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
        .await
        .map_err(WorkbenchError::Server)
}
