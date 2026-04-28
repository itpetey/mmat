use std::{net::SocketAddr, sync::Arc};

use axum::{Router, extract::ws::WebSocketUpgrade, response::Html, routing::get};
use dioxus::dioxus_core::VirtualDom;
use tokio::sync::{mpsc, oneshot, watch};
use tracing::info;

use crate::liveview::{
    assets::{APP_CSS, INDEX_HTML},
    components::{RootApp, RootAppProps},
    event::{EventReceiver, EventSender},
    state::{ProjectPrompt, UiState},
};

pub type InstructionReceiver = oneshot::Receiver<ProjectPrompt>;

const DEFAULT_ADDR: &str = "127.0.0.1:8080";
const LIVEVIEW_PATH: &str = "/liveview";

pub struct LiveViewHandle {
    shutdown_tx: watch::Sender<bool>,
    join_handle: tokio::task::JoinHandle<Result<(), LiveViewError>>,
}

pub struct LiveViewReadyHandle {
    shutdown_tx: watch::Sender<bool>,
    join_handle: tokio::task::JoinHandle<Result<(), LiveViewError>>,
    ready_rx: oneshot::Receiver<Result<(), std::io::Error>>,
}

pub struct LiveViewAppBuilder {
    addr: SocketAddr,
    ui_state: Arc<UiState>,
}

#[derive(Debug, thiserror::Error)]
pub enum LiveViewError {
    #[error("bind failed: {0}")]
    Bind(std::io::Error),
    #[error("server failed: {0}")]
    Serve(std::io::Error),
    #[error("task failed: {0}")]
    Task(String),
}

impl Default for LiveViewAppBuilder {
    fn default() -> Self {
        Self {
            addr: DEFAULT_ADDR
                .parse()
                .expect("default socket address should parse"),
            ui_state: Arc::new(UiState::new()),
        }
    }
}

impl LiveViewAppBuilder {
    pub fn addr(mut self, addr: SocketAddr) -> Self {
        self.addr = addr;
        self
    }

    pub fn with_ui_state(mut self, ui_state: Arc<UiState>) -> Self {
        self.ui_state = ui_state;
        self
    }

    pub fn spawn_with_input(
        self,
    ) -> Result<
        (
            EventSender,
            LiveViewReadyHandle,
            InstructionReceiver,
            EventReceiver,
        ),
        LiveViewError,
    > {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (instruction_tx, instruction_rx) = oneshot::channel();
        self.ui_state.prepare_initial_input(instruction_tx);
        let handle = spawn_server(self.addr, self.ui_state)?;
        Ok((event_tx, handle, instruction_rx, event_rx))
    }

    pub fn spawn(self) -> Result<LiveViewReadyHandle, LiveViewError> {
        spawn_server(self.addr, self.ui_state)
    }
}

impl LiveViewReadyHandle {
    pub async fn wait_for_ready(self) -> Result<LiveViewHandle, LiveViewError> {
        let ready_result = self
            .ready_rx
            .await
            .map_err(|_| LiveViewError::Task("server shut down before binding".into()))?;
        ready_result.map_err(LiveViewError::Bind)?;
        Ok(LiveViewHandle {
            shutdown_tx: self.shutdown_tx,
            join_handle: self.join_handle,
        })
    }
}

impl LiveViewHandle {
    pub async fn shutdown(self) -> Result<(), LiveViewError> {
        let _ = self.shutdown_tx.send(true);
        self.join_handle
            .await
            .map_err(|error| LiveViewError::Task(error.to_string()))?
    }
}

fn build_router(ui_state: Arc<UiState>) -> Router {
    let liveview_pool = dioxus_liveview::LiveViewPool::new();
    let liveview_state = ui_state.clone();

    Router::new()
        .route("/", get(move || async move { Html(index_html()) }))
        .route(
            LIVEVIEW_PATH,
            get(move |ws: WebSocketUpgrade| {
                let pool = liveview_pool.clone();
                let ui_state = liveview_state.clone();
                async move {
                    ws.on_upgrade(move |socket| async move {
                        let result = pool
                            .launch_virtualdom(dioxus_liveview::axum_socket(socket), move || {
                                VirtualDom::new_with_props(
                                    RootApp,
                                    RootAppProps {
                                        ui_state: ui_state.clone(),
                                    },
                                )
                            })
                            .await;
                        if let Err(error) = result {
                            tracing::debug!(target: "mmat::liveview", "liveview session ended: {error}");
                        }
                    })
                }
            }),
        )
}

fn index_html() -> String {
    INDEX_HTML.replace("__MMAT_APP_STYLES__", APP_CSS).replace(
        "__MMAT_LIVEVIEW_GLUE__",
        &dioxus_liveview::interpreter_glue(LIVEVIEW_PATH),
    )
}

async fn run_server(
    addr: SocketAddr,
    ui_state: Arc<UiState>,
    shutdown_rx: watch::Receiver<bool>,
    ready_tx: oneshot::Sender<Result<(), std::io::Error>>,
) -> Result<(), LiveViewError> {
    let app = build_router(ui_state);
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(error) => {
            let kind = error.kind();
            let _ = ready_tx.send(Err(std::io::Error::new(kind, error.to_string())));
            return Err(LiveViewError::Bind(error));
        }
    };
    let local_addr = listener.local_addr().map_err(LiveViewError::Bind)?;

    let _ = ready_tx.send(Ok(()));
    info!(target: "mmat::liveview", "LiveView UI listening on http://{local_addr}");

    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(wait_for_shutdown(shutdown_rx))
        .await
        .map_err(LiveViewError::Serve)
}

fn spawn_server(
    addr: SocketAddr,
    ui_state: Arc<UiState>,
) -> Result<LiveViewReadyHandle, LiveViewError> {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let (ready_tx, ready_rx) = oneshot::channel();
    let join_handle = tokio::spawn(run_server(addr, ui_state, shutdown_rx, ready_tx));

    Ok(LiveViewReadyHandle {
        shutdown_tx,
        join_handle,
        ready_rx,
    })
}

async fn wait_for_shutdown(mut shutdown_rx: watch::Receiver<bool>) {
    while !*shutdown_rx.borrow() {
        if shutdown_rx.changed().await.is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{net::SocketAddr, sync::Arc};

    use super::{LiveViewAppBuilder, UiState};

    #[tokio::test]
    async fn server_serves_liveview_shell() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .expect("ephemeral test listener should bind");
        let addr: SocketAddr = listener
            .local_addr()
            .expect("ephemeral listener should expose an address");
        drop(listener);

        let handle = LiveViewAppBuilder::default()
            .addr(addr)
            .with_ui_state(Arc::new(UiState::new()))
            .spawn()
            .expect("server should spawn")
            .wait_for_ready()
            .await
            .expect("server should bind");

        let mut stream = tokio::net::TcpStream::connect(addr)
            .await
            .expect("server should accept tcp connections");
        tokio::io::AsyncWriteExt::write_all(
            &mut stream,
            b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        )
        .await
        .expect("request should write");

        let mut response = String::new();
        tokio::io::AsyncReadExt::read_to_string(&mut stream, &mut response)
            .await
            .expect("response should read");

        assert!(response.contains("200 OK"));
        assert!(response.contains("/liveview"));
        assert!(response.contains("dioxus"));
        assert!(response.contains(".mmat-root"));
        assert!(!response.contains("__MMAT_APP_STYLES__"));
        assert!(!response.contains("__MMAT_LIVEVIEW_GLUE__"));

        handle.shutdown().await.expect("server should shut down");
    }
}
