## ADDED Requirements

### Requirement: Host work directory is configurable
The system SHALL accept a host work directory via the `MMAT_HOST_WORK_DIR` environment variable that defines the root directory under which all project directories reside.

#### Scenario: Host work directory set
- **WHEN** `MMAT_HOST_WORK_DIR` is set to a valid directory path
- **THEN** `OrganisationConfig::host_work_dir` MUST contain that path
- **AND** the system SHALL resolve all project directories relative to it

#### Scenario: Host work directory not set
- **WHEN** `MMAT_HOST_WORK_DIR` is not set
- **THEN** `OrganisationConfig::host_work_dir` MUST be `None`
- **AND** the system SHALL fall back to the current working directory for all operations

#### Scenario: Host work directory does not exist
- **WHEN** `MMAT_HOST_WORK_DIR` is set to a path that does not exist on the filesystem
- **THEN** the runtime startup SHALL fail with a clear error message indicating the missing directory

### Requirement: Worker resolves project path from host work dir
When a host work directory is configured, the Worker SHALL resolve the repository path by joining the host work directory with the project identifier from the event context.

#### Scenario: Worker resolves path with configured host dir
- **WHEN** `host_work_dir` is set to `/workspace` and `EventContext::project_id` is `my-app`
- **THEN** the Worker SHALL use `/workspace/my-app` as the repository root for worktree creation

#### Scenario: Worker falls back to cwd without host dir
- **WHEN** `host_work_dir` is `None`
- **THEN** the Worker SHALL use the process current working directory as the repository root

### Requirement: Worktree paths are under the resolved project directory
When the Worker creates worktrees, the worktree parent directory SHALL be the resolved project directory, not a hard-coded `"."`.

#### Scenario: Worktree created under project directory
- **WHEN** the Worker creates a worktree for a task in project `my-app`
- **AND** the host work dir is `/workspace`
- **THEN** the worktree SHALL be created under `/workspace/my-app/.worktrees/` (or `/workspace/my-app/.mmat-worktree-*` for fallback)
