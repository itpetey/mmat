## MODIFIED Requirements

### Requirement: Interactive mode starts LiveView UI instead of websocket protocol server
The interactive runtime entrypoint SHALL start a Dioxus LiveView host with shared `UiState` instead of the previous websocket-event-broadcast server. The runtime SHALL keep the server alive until Ctrl+C.

#### Scenario: Interactive mode starts without --prompt flag
- **WHEN** `mmat` is run with no `--prompt` argument
- **THEN** the LiveView server starts on the configured address
- **AND** an initial prompt input is stored in `UiState`
- **AND** the server URL is printed to stdout
- **AND** the process waits for Ctrl+C instead of shutting down after workflow completion

#### Scenario: Interactive mode starts with --prompt flag
- **WHEN** `mmat` is run with a `--prompt` argument
- **THEN** the LiveView server starts on the configured address
- **AND** no initial prompt input is stored in `UiState`
- **AND** the workflow begins immediately with the provided prompt
- **AND** the server URL is printed to stdout

#### Scenario: Workflow completion does not terminate server
- **WHEN** the MMAT workflow completes (success or error)
- **THEN** the LiveView server remains active
- **AND** the transcript remains visible in the browser
- **AND** the process waits for Ctrl+C

#### Scenario: Ctrl+C terminates the server
- **WHEN** the user sends Ctrl+C to the terminal
- **THEN** a `FrontendEvent::Quit` is sent to the UiState receiver
- **AND** the server shuts down gracefully
