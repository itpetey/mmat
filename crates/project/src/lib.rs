//! Project crate providing worktree isolation, repository discovery,
//! directory scaffolding, and git operations.

/// Re-export of the repository discovery and project type detection logic.
pub use discovery::RepoDiscovery;
/// Re-export of the directory scanning logic for project discovery.
pub use discovery::discover_projects;
/// Re-export of the project scaffolding and directory structure creation.
pub use scaffold::ProjectScaffold;
/// Re-export of the git worktree isolation and lifecycle management.
pub use worktree::WorktreeHandle;

pub mod discovery;
pub mod scaffold;
pub mod worktree;
