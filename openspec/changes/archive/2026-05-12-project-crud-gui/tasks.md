## 1. Add project lifecycle event variants

- [x] 1.1 Add `ProjectListed` variant to `SemanticEvent` enum with `project_id`, `path` fields and a `new_project_listed()` constructor
- [x] 1.2 Add `ProjectRenamed` variant to `SemanticEvent` enum with `project_id`, `old_name`, `new_name` fields and a `new_project_renamed()` constructor
- [x] 1.3 Add `ProjectDeleted` variant to `SemanticEvent` enum with `project_id`, `name` fields and a `new_project_deleted()` constructor
- [x] 1.4 Ensure new variants implement `Serialize`/`Deserialize` and are handled in any existing match arms that exhaust `SemanticEvent` variants

## 2. Extend WorkbenchProjection to track all projects

- [x] 2.1 Add `projects: Vec<ProjectView>` field to `WorkbenchProjection` struct in `app.rs`
- [x] 2.2 Implement `apply_event` handling for `ProjectCreated`: add to `projects` list and set as active project
- [x] 2.3 Implement `apply_event` handling for `ProjectListed`: add to `projects` list if not already present
- [x] 2.4 Implement `apply_event` handling for `ProjectRenamed`: update project name in `projects` list
- [x] 2.5 Implement `apply_event` handling for `ProjectDeleted`: remove from `projects` list, clear active project if deleted project was active

## 3. Add project listing and management API endpoints

- [x] 3.1 Add `GET /api/projects` handler that returns JSON array from `projection.projects`
- [x] 3.2 Add `GET /api/projects/{id}` handler that returns a single project or 404
- [x] 3.3 Update `POST /api/projects` handler to check uniqueness against existing project names (return 409 on conflict)
- [x] 3.4 Add `PATCH /api/projects/{id}` handler that renames project on disk, publishes `ProjectRenamed` event, updates projection
- [x] 3.5 Add `DELETE /api/projects/{id}` handler that removes project directory from disk, publishes `ProjectDeleted` event, clears active project if needed
- [x] 3.6 Add `POST /api/projects/{id}/select` handler that sets active project in projection
- [x] 3.7 Register all new routes in `build_app_router()`

## 4. Implement project discovery from filesystem

- [x] 4.1 Add project discovery function in the `project` crate that scans `MMAT_HOST_WORK_DIR` and returns a list of valid project directories
- [x] 4.2 Implement validation logic: a directory is a valid project if it contains a recognisable project structure (e.g., `Cargo.toml` for Rust, `package.json` for Node)
- [x] 4.3 Call discovery during `startup_projection()` in `main.rs`, publishing `ProjectListed` events for each discovered directory not already in the projection

## 5. Implement project sidebar in the frontend

- [x] 5.1 Add project sidebar HTML markup to `index.html` (`<nav id="project-sidebar">` with project list, create button)
- [x] 5.2 Add project sidebar styles to `style.css` (layout, active state highlighting, action button visibility)
- [x] 5.3 Implement project list rendering in `app.js` from the state snapshot's `projects` array
- [x] 5.4 Implement project switching: clicking a non-active project sends `POST /api/projects/{id}/select`
- [x] 5.5 Implement project creation flow: button shows name input, on submit sends `POST /api/projects`
- [x] 5.6 Implement project rename: inline edit or prompt on rename action, sends `PATCH /api/projects/{id}`
- [x] 5.7 Implement project deletion: delete action shows confirmation dialog, on confirm sends `DELETE /api/projects/{id}`
- [x] 5.8 Handle SSE events for project lifecycle updates (new project appears, rename updates name, delete removes entry)
- [x] 5.9 Ensure keyboard accessibility: tab order includes sidebar, Enter activates projects, actions are keyboard-reachable

## 6. Verification and polish

- [x] 6.1 Run `cargo fmt --all` and `cargo clippy -- -D warnings` and fix any issues
- [x] 6.2 Run `cargo test` and fix any regressions
- [x] 6.3 Manually verify: create multiple projects, switch between them, rename, delete, confirm deleted project's directory is removed from disk
- [x] 6.4 Verify project discovery: create a project directory manually in host work dir, restart workbench, confirm it appears in the sidebar
- [x] 6.5 Verify `/api/state` response includes both `project` (active) and `projects` (all) fields
