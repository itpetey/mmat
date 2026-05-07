## ADDED Requirements

### Requirement: Shell commands can be constructed and executed
The system SHALL provide a `ProcessCommand` type that wraps a shell command string with optional working directory and environment variables. Execution MUST return a `ProcessOutput` containing stdout bytes, stderr bytes, and exit code. Commands MUST be executed via `tokio::process::Command`.

#### Scenario: Successful command execution
- **WHEN** `ProcessCommand::shell("echo hello").execute()` is called
- **THEN** the returned `ProcessOutput` MUST have `exit_code == 0`
- **AND** `stdout` MUST contain `"hello\n"` (as bytes)
- **AND** `stderr` MUST be empty

#### Scenario: Failed command execution
- **WHEN** `ProcessCommand::shell("exit 1").execute()` is called
- **THEN** the returned `ProcessOutput` MUST have `exit_code == 1`

#### Scenario: Command with working directory
- **WHEN** `ProcessCommand::shell("pwd").with_working_dir("/tmp").execute()` is called
- **THEN** the command MUST execute with `/tmp` as its working directory
- **AND** stdout MUST contain `/tmp` (or the resolved path)

### Requirement: Process output provides string conversion
The system SHALL provide convenience methods on `ProcessOutput` to interpret stdout and stderr as UTF-8 strings.

#### Scenario: UTF-8 stdout is decoded
- **WHEN** a `ProcessOutput` has valid UTF-8 stdout bytes
- **THEN** `output.stdout_str()` MUST return `Ok(&str)` containing the decoded text

#### Scenario: Non-UTF-8 stdout produces error
- **WHEN** a `ProcessOutput` has invalid UTF-8 stdout bytes
- **THEN** `output.stdout_str()` MUST return `Err` with the UTF-8 decode error

### Requirement: Process output is serializable for evidence logging
The system SHALL implement `Serialize` and `Deserialize` for `ProcessOutput` so that command results can be stored in the event store as `ToolExecuted` evidence.

#### Scenario: Process output round-trips through JSON
- **WHEN** a `ProcessOutput` is serialized to JSON and deserialized back
- **THEN** the `exit_code`, `stdout`, and `stderr` MUST match the original values
