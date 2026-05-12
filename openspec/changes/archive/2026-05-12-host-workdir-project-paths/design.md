## Context

The system currently has no configurable host-level working directory. The Worker hardcodes `Path::new(".")` as the repository root (`crates/roles/src/worker.rs:594`), and projects exist only as logical string identifiers (`EventContext::project_id`). There is no API to create projects or assign them filesystem locations. When running inside Docker, a mounted host directory must serve as the root from which all project directories are resolved.

**Current state:**
- `OrganisationConfig` has no path configuration fields
- `EventContext` carries `project_id: String` (hard-coded to `"project-workbench-mvp"`) but no filesystem path
- `AppState::publish` always uses `"default-organisation"` / `"default-workspace"` literals
- Worker creates worktrees under `"."` regardless of project identity
- `RepositoryOutputRef::repository_path` is always `"."`
- No project creation API exists; the workbench projection seeds a single hard-coded project

## Goals / Non-Goals

**Goals:**
- Define a host work directory that serves as the root for all project directories
- Allow projects to be created with a directory name that resolves under the host work dir
- Update the Worker to use the project directory (or cwd when unconfigured) as the repository root
- Keep backward compatibility: when `host_work_dir` is not set, the system behaves as today

**Non-Goals:**
- Multi-organisation or multi-workspace directory layout
- Migration of existing worktrees or data for existing projects
- Docker Compose / container orchestration configuration (this is deployment, not code)

## Decisions

### 1. `OrganisationConfig` gains `host_work_dir: Option<PathBuf>`

Set via `MMAT_PROJECT_DIR` environment variable. When `None`, the system falls back to the current working directory — preserving backward compatibility for local development.

**Alternatives considered:**
- CLI flag: rejected because the workbench binary has no CLI argument parsing and adding one adds complexity without benefit over env vars
- A separate `HostConfig` struct: rejected because the host work dir is naturally part of the organisation/runtime configuration
- Per-project absolute paths: rejected because constraining paths under a common root simplifies Docker mounts

### 2. Project identity maps to directory path

A project's directory name IS the project identifier. `POST /api/projects` accepts `{ "name": "my-app" }` which creates directory `<host_work_dir>/my-app`. The `project_id` in `EventContext` becomes the project name/directory name (e.g. `"my-app"`).

**Rationale:** A single string serves as both identifier and directory name, avoiding two separate fields that would always need to stay in sync. Project names are validated to be valid directory names (alphanumeric + hyphens/underscores, no path separators).

**Alternatives considered:**
- Separate `project_path` field on `EventContext`: adds complexity without benefit — why would a project have an ID different from its directory name?
- UUID-based project IDs with a name→path mapping table: premature abstraction for single-machine operation

### 3. Worker resolves repository path from runtime config and event context

At task execution time, the Worker reads the host work dir from the runtime config (passed via `RoleContext`) and the project path from `EventContext::project_id`. Resolution logic:

```
if let Some(ref host) = host_work_dir {
    host.join(&event_context.project_id)
} else {
    PathBuf::from(".")
}
```

**Rationale:** Clean separation — the host directory is a runtime-level concern, the project identity is an event-level concern. The Worker just needs both to resolve a path, and this avoids threading a project-path through contract/task types.

### 4. Workbench gets a `POST /api/projects` endpoint

Accepts `{ "name": "my-app" }`. Validates the name is a valid directory name. Creates the directory under `host_work_dir` (or fails with a clear error if `host_work_dir` is not configured). Populates `project_id` in the projection and publishes a `ProjectCreated` event (new `SemanticEvent` variant).

**Rationale:** Project creation is an explicit user action, not automatic. This keeps the workbench simple and gives the user control over when projects exist.

### 5. Backward compatibility: unconfigured host dir preserves cwd behaviour

When `MMAT_PROJECT_DIR` is not set, `OrganisationConfig::host_work_dir` is `None`. The Worker defaults to `Path::new(".")`. The workbench continues to use `"project-workbench-mvp"` as the project ID (which resolves to `./project-workbench-mvp` relative path, or `"."` when no project is selected — maintaining the existing behaviour).

## Risks / Trade-offs

- **Risk:** Docker mount path does not exist → startup fails. **Mitigation:** The binary validates `host_work_dir` exists and is a directory on startup, with a clear error message.
- **Risk:** Project directory name conflicts with existing files or reserved names → **Mitigation:** Validate on creation; reject names containing path separators, `.`, `..`, or shell metacharacters.
- **Risk:** Worker runs in the wrong directory if `host_work_dir` is misconfigured → **Mitigation:** Log the resolved working directory at task start for observability.
- **Trade-off:** Requiring `host_work_dir` to be set for Docker deployment adds a setup step. **Acceptable** because the Docker Compose configuration will document the env var and mount.

## Open Questions

- Should the workbench UI allow selecting/creating projects, or is this a CLI/API-only operation initially? **Decision deferred to implementation — start with API-only.**
