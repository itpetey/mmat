# workbench-packaging Specification

## Purpose
TBD - created by syncing change workbench-packaging-operations. Update Purpose after archive.
## Requirements
### Requirement: Workbench run configuration is documented
The project SHALL document all required and optional environment variables for running the workbench.

#### Scenario: Developer reads README
- **WHEN** a developer reads the workbench usage section
- **THEN** it MUST list `MMAT_DB_URL` as required
- **AND** `MMAT_WORKBENCH_ADDR` as optional

### Requirement: Workbench startup errors are actionable
The workbench SHALL return clear errors for missing configuration, bind failures, missing assets, and persistence failures.

#### Scenario: Port is occupied
- **WHEN** the configured bind address is already in use
- **THEN** startup MUST fail with an error mentioning the address and bind failure

#### Scenario: Static assets are embedded at compile time
- **GIVEN** the workbench uses `include_str!()` to embed HTML, CSS, and JavaScript into the binary
- **WHEN** a required static asset file is missing from the source tree
- **THEN** compilation fails with a compiler error containing the missing file path
- **AND** no runtime asset directory is needed at startup

### Requirement: Release builds serve the workbench
The documented release build SHALL produce a workbench executable that can serve the UI and API from the documented working directory.

#### Scenario: Release run starts
- **WHEN** a user runs the release workbench binary with required configuration
- **THEN** `/`, `/api/state`, and `/events` MUST be available

### Requirement: Workbench maturity label is accurate
Documentation SHALL describe the workbench maturity consistently as prototype or MVP according to implemented capabilities.

#### Scenario: MVP criteria are not complete
- **WHEN** Postgres-only persistence or static assets are incomplete
- **THEN** docs MUST continue to call the workbench a prototype or experimental workbench

#### Scenario: MVP criteria are complete
- **WHEN** Postgres-only persistence, separated assets, restart replay, and core tests are complete
- **THEN** docs MAY describe it as an MVP frontend
