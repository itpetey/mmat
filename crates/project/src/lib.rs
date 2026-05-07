//! Project crate providing worktree isolation, repository discovery,
//! directory scaffolding, and git operations.

pub use discovery::RepoDiscovery;
pub use scaffold::ProjectScaffold;
pub use worktree::WorktreeHandle;

pub mod discovery;
pub mod scaffold;
pub mod worktree;
