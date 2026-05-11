## Why

Artefact storage must distinguish transient/non-code artefacts from generated code. Non-code artefacts belong in Postgres blob storage; generated code belongs in the project repository or an isolated worktree.

## What Changes

- Route transient and structured artefacts through Postgres artefact/blob storage.
- Route produced code to project repositories/worktrees only.
- Expose artefact metadata, evidence, and repository paths in the workbench.
- Remove workbench-local artefact filesystem assumptions.

## Capabilities

### New Capabilities
- `project-artefact-output`: Artefact storage and generated-code output routing for workbench-driven delivery.

### Modified Capabilities

## Impact

- Affects artefact store usage, Worker outputs, workbench artefact inspection, repository/worktree handling, and tests.
