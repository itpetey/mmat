## Why

The workbench GUI currently supports only creating a single active project via `POST /api/projects`, with no ability to list, switch between, rename, or delete projects. Users working on multiple projects must manually manage project directories on disk, breaking the GUI workflow. This change brings full project lifecycle management into the GUI, making the workbench a true multi-project tool.

## What Changes

- Add a project list API (`GET /api/projects`) returning all known projects with metadata (name, path, run count, last activity, status)
- Add project detail API (`GET /api/projects/{id}`) for retrieving a single project's metadata
- Add project rename API (`PATCH /api/projects/{id}`) for updating project name
- Add project delete API (`DELETE /api/projects/{id}`) with confirmation, removing the project directory from disk
- Add project activation/selection API (`POST /api/projects/{id}/select`) to switch the active project in the GUI
- Add a `ProjectListed` semantic event variant to track project discovery
- Add a `ProjectDeleted` semantic event variant to track project removal
- Extend the `WorkbenchProjection` with a `projects` collection to track all known projects (not just the active one)
- Add a project listing/sidebar UI component to the frontend (`index.html` + `app.js` + `style.css`)
- **BREAKING**: `/api/state` response shape changes — `project` field remains the active project, new `projects` field lists all known projects
- **BREAKING**: `POST /api/projects` now requires a unique project name (previously checked on disk only)

## Capabilities

### New Capabilities

- `project-list-api`: HTTP API endpoints for listing, inspecting, renaming, and deleting projects
- `project-discovery`: Filesystem scanning to discover and index existing project directories as known projects
- `project-lifecycle-events`: New semantic event variants (`ProjectListed`, `ProjectRenamed`, `ProjectDeleted`) for the project lifecycle beyond creation
- `project-ui`: Frontend sidebar/selector component for browsing and switching between projects, with project management actions (rename, delete)

### Modified Capabilities

- `workbench-core`: The `WorkbenchProjection` gains a `projects` collection alongside the existing `project` field; `/api/state` response shape changes
- `semantic-event-types`: New event variants added to the `SemanticEvent` enum
- `workbench-frontend-ux`: New UI component for project listing, creation, and management added to the GUI

## Impact

- **`crates/workbench/src/app.rs`**: New API routes, updated projection struct, new projection event handlers
- **`crates/event-stream/src/event.rs`**: New `SemanticEvent` variants and constructor methods
- **`crates/workbench/static/index.html`**: New project sidebar/selector markup
- **`crates/workbench/static/app.js`**: New project management UI logic
- **`crates/workbench/static/style.css`**: New project sidebar/selector styles
- **`crates/project/src/`**: New project discovery (filesystem scan) logic
- **`openspec/specs/workbench-core/spec.md`**: Updated projection requirements
- **`openspec/specs/semantic-event-types/spec.md`**: New event variant requirements
- **`openspec/specs/workbench-frontend-ux/spec.md`**: New UI component requirements
