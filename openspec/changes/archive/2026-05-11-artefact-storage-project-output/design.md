## Context

The project already has Postgres artefact storage specs. The workbench should not place transient artefacts under a local data directory. Code is different: it must be materialised in the actual project repository/worktree so users can inspect, test, and commit it.

## Goals / Non-Goals

**Goals:**
- Store non-code artefacts in Postgres with metadata and evidence links.
- Write generated code to repository/worktree paths.
- Show both blob artefacts and repository outputs in the UI.

**Non-Goals:**
- Storing entire repository snapshots as blobs.
- Automatically committing generated code.

## Decisions

- Use `db://artefacts/{id}` for non-code artefact references.
- Use repository-relative paths and worktree metadata for generated code references.
- Keep evidence refs attached to both stored artefacts and repository outputs.

## Risks / Trade-offs

- Large blobs in Postgres can grow quickly. Mitigation: rely on TOAST initially and add retention/archival later.
- Repository writes are riskier than blob writes. Mitigation: use isolated worktrees and visible safety/validation state.
