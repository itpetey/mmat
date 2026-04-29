//! Browser LiveView UI for interactive MMAT runs.

pub use event::{EventReceiver, EventSender, FrontendEvent, RunSummaryEvent};
pub use logging::init_liveview_tracing;
pub use server::{
    InstructionReceiver, LiveViewAppBuilder, LiveViewError, LiveViewHandle, LiveViewReadyHandle,
};
pub use state::{
    BuildJobSnapshot, ComposerMode, ConversationEntry, ConversationHistoryError,
    ConversationHistoryStore, PendingPrompt, PendingPromptSnapshot, ProjectPrompt,
    ProjectWorkerSnapshot, RunSummary, UiEvent, UiEventEntry, UiSnapshot, UiState,
};
pub use translator::spawn_event_translator;

mod assets;
mod components;
mod event;
mod logging;
mod server;
mod state;
mod translator;
