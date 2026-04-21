## ADDED Requirements

### Requirement: LiveView server hosts the browser UI
The system SHALL serve a Dioxus LiveView application at the root path (`/`) that renders the MMAT chat interface. The server SHALL use Axum as the underlying HTTP framework and Dioxus LiveView's Axum adapter for websocket transport.

#### Scenario: Browser loads the LiveView page
- **WHEN** a browser navigates to `http://127.0.0.1:PORT/`
- **THEN** the server returns an HTML page containing Dioxus's `interpreter_glue` script
- **AND** the script establishes a websocket connection to the LiveView endpoint

#### Scenario: LiveView websocket endpoint is served
- **WHEN** the browser's interpreter glue connects to the LiveView websocket route
- **THEN** Dioxus LiveView accepts the connection and begins syncing VirtualDom updates

### Requirement: LiveView host shares UiState with workflow runtime
The LiveView host SHALL accept a shared `UiState` reference that is also accessible to the workflow runtime and tracing layer. The LiveView components SHALL read from and write to this shared state.

#### Scenario: LiveView reads current transcript on connect
- **WHEN** a new LiveView session connects
- **THEN** the root component reads the current `event_history` from `UiState`
- **AND** renders all existing transcript entries

#### Scenario: LiveView reflects pending prompt state
- **WHEN** `UiState.pending_prompt` is `Some`
- **THEN** the LiveView UI renders a prompt card with the question and reply input
- **AND** submitting a reply writes the response to the stored oneshot sender

### Requirement: Server serves static CSS assets
The server SHALL serve a plain CSS stylesheet for the chat UI. Static assets SHALL be served from a configurable directory or embedded path.

#### Scenario: CSS stylesheet is served
- **WHEN** the browser requests `/style.css`
- **THEN** the server returns the stylesheet with `text/css` content type

### Requirement: Server stays alive until Ctrl+C
The LiveView server SHALL remain running after workflow completion and SHALL only shut down when receiving a `SIGINT` / `Ctrl+C` signal.

#### Scenario: Server persists after workflow completes
- **WHEN** the MMAT workflow finishes or errors out
- **THEN** the HTTP and websocket servers remain active
- **AND** the browser UI continues to display the final transcript

#### Scenario: Server shuts down on Ctrl+C
- **WHEN** the user sends `Ctrl+C` to the terminal
- **THEN** the server sends a quit event and waits for graceful shutdown
- **AND** all connections are closed cleanly
