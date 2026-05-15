use crate::ui::App;

mod api;
#[cfg(feature = "server")]
mod cli;
mod ui;

#[cfg(feature = "web")]
fn main() {
    dioxus::launch(App);
}

#[cfg(feature = "server")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use dioxus::server::{DioxusRouterExt, ServeConfig};

    let cfg = cli::load_config()?;

    let router = axum::Router::new().serve_dioxus_application(ServeConfig::new(), App);
    let bind_addr = if dioxus::cli_config::is_cli_enabled() {
        dioxus::cli_config::fullstack_address_or_localhost().to_string()
    } else {
        cfg.bind_addr
    };
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}
