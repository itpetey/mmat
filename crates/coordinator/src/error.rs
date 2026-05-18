//! Error types for the coordinator crate.

use mmat_db::RunError;
use thiserror::Error;

/// Alias for a `Result` with the crate's [`enum@Error`] type.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur during coordinator operations.
#[derive(Error, Debug)]
pub enum Error {
    /// The requested role was not found in the registry.
    #[error("Role not found: {0}")]
    RoleNotFound(String),

    /// A role with the same ID is already registered.
    #[error("Duplicate role ID: {0}")]
    DuplicateRoleId(String),

    /// The role specification is invalid or incomplete.
    #[error("Invalid role spec: {0}")]
    InvalidRoleSpec(String),

    /// A role violated its contract.
    #[error("Contract violation: {0}")]
    ContractViolation(String),

    /// A role's time or token budget was exceeded.
    #[error("Budget exceeded: {0}")]
    BudgetExceeded(String),

    /// Escalation to a higher-authority role is required.
    #[error("Escalation required: {0}")]
    EscalationRequired(String),

    /// An I/O operation failed.
    #[error("IO error: {0}")]
    Io(String),

    /// A general runtime error occurred.
    #[error("Runtime error: {0}")]
    Runtime(String),

    #[error("Database error: {0}")]
    Database(#[from] RunError),
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err.to_string())
    }
}
