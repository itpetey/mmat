## Context

The MMAT workbench uses event sourcing — every state change flows through `SemanticEvent` variants published to the `EventBus`. The `WorkbenchProjection` (an in-memory read model in `app.rs`) applies each event to build the UI's view of the world. There is no separate "project" database table; project state is derived from events.

Currently, the workbench tracks only a single active project (`WorkbenchProjection.project: ProjectView`). The `POST /api/projects` handler creates a directory on disk, publishes `ProjectCreated`, and sets the active project. But there is no way to list, switch, rename, or delete projects.

The host work directory (`MMAT_HOST_WORK_DIR`) may contain multiple project directories from prior sessions or manual creation. These are invisible to the GUI.

## Goals / Non-Goals

**Goals:**
- Allow users to list all known projects with metadata (name, path, run count, last activity)
- Allow users to switch the active project from the GUI
- Allow users to rename and delete projects
- Discover existing project directories on the filesystem at startup
- Keep the event sourcing pattern — all project lifecycle changes are events

**Non-Goals:**
- Project import/export or sharing
- Project templates or scaffolding changes (scaffolding exists in `project` crate, unchanged)
- Multi-user or authentication concerns
- Server-side project backup or recovery
- Project-level configuration beyond name and path

## Decisions

### 1. Projects tracked via events + projection, not a DB table

No new `projects` table. The event store already persists all project lifecycle events (`ProjectCreated` exists, we add `ProjectListed`, `ProjectRenamed`, `ProjectDeleted`). The `WorkbenchProjection.projects: Vec<ProjectView>` field is built by replaying these events at startup.

**Rationale**: Consistent with the existing event sourcing architecture. No migration needed. No dual-write consistency issues between a projects table and event stream.

**Alternatives considered**: A dedicated `projects` table queried directly by the API. Rejected because it would introduce a second source of truth and require synchronisation with events.

### 2. Project ID is the directory name (slug)

Project identity is the filesystem directory name, derived from the project name by lowercasing and replacing non-alphanumeric characters with hyphens (slugification). The project name (display name) is stored alongside but the ID/slug is the canonical identifier.

**Rationale**: The `ProjectCreated` event already uses `project_id` as the directory name. Keeping this identity scheme avoids breaking existing event history. The slug is guaranteed to be a valid filesystem name.

**Alternatives considered**: UUID-based project IDs with name as metadata. Rejected because the filesystem directory is the primary project container, and switching to UUIDs would break all existing `project_id` references in the event store.

### 3. Discovery at startup via `startup_projection`

Existing project directories in `MMAT_HOST_WORK_DIR` are discovered at startup during `startup_projection()` in `main.rs`. For each directory that is a valid project (has the expected structure), a `ProjectListed` event is created and applied to the projection. Directories that already match a known project (from a prior `ProjectCreated` event) are skipped as duplicates.

**Rationale**: This is the only reliable time to scan the filesystem. Doing it per-request would be slow and racy. Startup discovery ensures the projection always reflects actual disk state.

**Alternatives considered**: Adding a `GET /api/projects/scan` endpoint for on-demand discovery. Rejected — adds unnecessary API surface and creates inconsistency between disk and projection state if the user forgets to scan.

### 4. Delete means remove from disk + mark deleted in projection

`DELETE /api/projects/{id}` removes the project directory from disk (via `std::fs::remove_dir_all`) and emits a `ProjectDeleted` event. The projection applies this event by removing the project from `projects` and clearing `project` if the deleted project was active. Past events for the deleted project remain in the event store for audit.

**Rationale**: This matches the user's expectation of "delete the project". Soft-delete (keeping the directory) would accumulate stale directories. The event store retains the audit trail.

**Alternatives considered**: Move to a `.deleted/` subdirectory (trash/recycle). Rejected as overcomplicating — adds state management for trash, and the user can simply recreate a project.

### 5. Project sidebar as vanilla JS component

The frontend addition follows the existing pattern: no framework, no build step. A new `<nav id="project-sidebar">` section in `index.html`, styled in `style.css`, with logic in `app.js`. The sidebar renders from the `projects` field in the SSE state snapshot and responds to `ProjectCreated`/`ProjectRenamed`/`ProjectDeleted` events.

**Rationale**: The codebase has no JavaScript framework or build tooling. Introducing one (React, htmx) would be a separate, larger change. The vanilla JS pattern is already established.

**Alternatives considered**: Adding htmx for partial updates. Rejected because the SSE-based architecture already provides the reactivity pattern without additional libraries.

## Risks / Trade-offs

**[Risk] Project discovery at startup may be slow with many directories** → Mitigation: `MMAT_HOST_WORK_DIR` typically contains a small number of projects (single-digit). If this becomes a problem, discovery can be made async and non-blocking, populating the projection lazily.

**[Risk] Concurrent project deletion and activation could leave the projection in an inconsistent state** → Mitigation: The projection is guarded by an `Arc<RwLock<>>` (exclusive write access). All mutation handlers run sequentially within the projection update loop. The API handler acquires the write lock before mutating.

**[Risk] Deleted project events remain in store forever (no GC)** → Mitigation: This is consistent with the append-only event store design. Events are small (KB range). If GC is needed, it should be a separate change targeting the event store layer, not project-specific.

**[Risk] Renaming a project on disk while the workbench is running breaks consistency** → Mitigation: Document that project directories should not be manually modified while the workbench is running. The `PATCH` API provides the safe rename path.

## Open Questions

- Should we support archiving projects (keep directory, remove from active list) separately from deletion? Currently out of scope — delete is the primary need.
- Should the project sidebar show run count or last activity? These can be derived from events but may be expensive to compute. Start with simple metadata (name, path, status) and add derived fields later.
