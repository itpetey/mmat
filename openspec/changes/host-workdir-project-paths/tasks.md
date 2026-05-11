## 1. OrganisationConfig — host work directory

- [ ] 1.1 Add `host_work_dir: Option<PathBuf>` field to `OrganisationConfig` in `crates/coordinator/src/runtime.rs`
- [ ] 1.2 Add `host_work_dir: None` to the `Default` impl for `OrganisationConfig`
- [ ] 1.3 Read `MMAT_HOST_WORK_DIR` env var in `build_runtime` (`crates/workbench/src/app.rs`) and populate `OrganisationConfig::host_work_dir`
- [ ] 1.4 Validate that `host_work_dir` exists and is a directory at startup (in `build_runtime`), failing with a clear error if set to a non-existent path

## 2. RoleContext — thread host work dir to roles

- [ ] 2.1 Add `host_work_dir: Option<PathBuf>` field to `RoleContext` in `crates/coordinator/src/role.rs`
- [ ] 2.2 Populate `RoleContext::host_work_dir` from `OrganisationConfig::host_work_dir` in `OrganisationRuntime::run` (`crates/coordinator/src/runtime.rs:220`)

## 3. SemanticEvent — ProjectCreated variant

- [ ] 3.1 Add `ProjectCreated` variant to `SemanticEvent` enum in `crates/event-stream/src/event.rs` with fields: `event_id`, `source_agent`, `timestamp_ns`, `context`, `project_id`, `host_work_dir`
- [ ] 3.2 Add `ProjectCreated` variant to `EventType` enum in `crates/event-stream/src/event.rs`
- [ ] 3.3 Add factory method `SemanticEvent::new_project_created(source_agent, project_id, host_work_dir)`
- [ ] 3.4 Update `event_id()` match arm for `ProjectCreated`
- [ ] 3.5 Update `event_type_str()` match arm for `ProjectCreated`
- [ ] 3.6 Update `event_type()` match arm for `ProjectCreated`
- [ ] 3.7 Update `with_context()` match arm for `ProjectCreated`
- [ ] 3.8 Update `Display` impl for `EventType` with `ProjectCreated`
- [ ] 3.9 Update event name/type mapping if a separate lookup table exists

## 4. Worker — resolve project path from host work dir

- [ ] 4.1 In Worker's `run_loop`, read `host_work_dir` from `RoleContext` and `project_id` from `EventContext` on incoming `TaskAssigned` events
- [ ] 4.2 Replace `Path::new(".")` (line 594) with resolution logic: if host_work_dir is Some, join with project_id; else use cwd
- [ ] 4.3 Log the resolved repository path at task start for observability
- [ ] 4.4 Ensure `RepositoryOutputRef::repository_path` in `publish_artefact` reflects the resolved project path (it already uses `worktree.repo_path().display()` which will be correct once the Worker creates the worktree under the right path)

## 5. Workbench — POST /api/projects endpoint

- [ ] 5.1 Define request/response types — `CreateProjectRequest { name: String }`, `CreateProjectResponse { id: String, name: String, path: String }` — in `crates/workbench/src/app.rs`
- [ ] 5.2 Add `POST /api/projects` route to `build_app_router` in `crates/workbench/src/app.rs`
- [ ] 5.3 Implement `create_project` handler: validate name (alphanumeric + hyphens/underscores, no path separators, no `.` or `..`), reject if empty
- [ ] 5.4 In `create_project` handler: resolve full path as `host_work_dir.join(name)`, fail if host_work_dir is not configured
- [ ] 5.5 In `create_project` handler: check for existing directory, fail with conflict error if it exists
- [ ] 5.6 In `create_project` handler: create the directory via `tokio::fs::create_dir`
- [ ] 5.7 In `create_project` handler: update projection — set `active_project_id` to the new project name, populate `ProjectView` fields
- [ ] 5.8 In `create_project` handler: publish `SemanticEvent::new_project_created(...)` with the project_id and host_work_dir

## 6. Workbench projection — handle ProjectCreated event

- [ ] 6.1 In `WorkbenchProjection::apply` (`crates/workbench/src/app.rs`), add match arm for `ProjectCreated`: update `project.id` and `project.name` from the event
- [ ] 6.2 Store `host_work_dir` in the projection if needed (or in `AppState`) so the UI can display the project path
- [ ] 6.3 Ensure `active_project_id` updates correctly when a new project is created or an existing one is selected

## 7. Tests

- [ ] 7.1 Test `OrganisationConfig::default()` does not include a host_work_dir (backward compatibility)
- [ ] 7.2 Test Worker resolves repo path correctly with host_work_dir + project_id
- [ ] 7.3 Test Worker falls back to cwd when host_work_dir is None
- [ ] 7.4 Test `create_project` handler rejects invalid project names
- [ ] 7.5 Test `create_project` handler rejects when host_work_dir is not configured
- [ ] 7.6 Test `create_project` handler creates directory and publishes ProjectCreated
- [ ] 7.7 Test workbench projection applies ProjectCreated events
- [ ] 7.8 Test `SemanticEvent::new_project_created` round-trips through serialization

## 8. Verification

- [ ] 8.1 Run `cargo fmt --all`
- [ ] 8.2 Run `cargo clippy -- -D warnings`
- [ ] 8.3 Run `cargo test`
