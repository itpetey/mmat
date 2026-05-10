## ADDED Requirements

### Requirement: Runtime supports Postgres-only workbench mode
The runtime SHALL expose configuration suitable for workbench usage where Postgres-backed event, memory, and artefact stores are mandatory.

#### Scenario: Workbench mode rejects SQLite store paths
- **WHEN** the workbench constructs runtime configuration
- **THEN** `database_url` MUST be populated
- **AND** legacy SQLite event or memory store paths MUST NOT be used

#### Scenario: Store initialisation failure stops runtime creation
- **WHEN** Postgres event, memory, or artefact store initialisation fails
- **THEN** runtime construction MUST return an error
- **AND** no roles MUST be spawned
