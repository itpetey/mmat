## Why

The system has no configurable working directory. The Worker hardcodes `Path::new(".")` as the repository root, projects are not real directories, and there is no concept of a host-side workspace where all projects live. When running inside Docker, a mounted host directory must become the root from which project directories are resolved.

## What Changes

- Introduce a `host_work_dir` configuration on `OrganisationConfig` (with a `MMAT_PROJECT_DIR` env var override) that defines the root directory where all projects reside.
- Add a `project_path` field to project identity — when a project is created, it specifies a directory name under the host work dir (e.g. `my-app` resolves to `/host/work/dir/my-app`).
- Update the Worker to resolve working directories from the project path instead of unconditionally using `"."`.
- Wire the project path through `EventContext` so roles receive it at runtime.
- Add a `POST /api/projects` endpoint to the workbench for creating projects with a name and path.

## Capabilities

### New Capabilities
- `host-configuration`: Configuration of a host work directory as the root for all project directories.
- `project-creation`: Projects are created with a directory path resolved under the host work dir.

### Modified Capabilities

## Impact

- Affects `OrganisationConfig`, `EventContext`, `AppState`, workbench HTTP API, Worker role (`worker.rs`), worktree creation, `RepositoryOutputRef`, and any code that assumes `"."` as the repository root.
- **BREAKING**: The Worker will no longer work without a configured host work dir and project path.
