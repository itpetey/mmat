use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug, Clone)]
pub enum Error {
    #[error("Role not found: {0}")]
    RoleNotFound(String),

    #[error("Duplicate role ID: {0}")]
    DuplicateRoleId(String),

    #[error("Invalid role spec: {0}")]
    InvalidRoleSpec(String),

    #[error("Contract violation: {0}")]
    ContractViolation(String),

    #[error("Budget exceeded: {0}")]
    BudgetExceeded(String),

    #[error("Escalation required: {0}")]
    EscalationRequired(String),

    #[error("IO error: {0}")]
    Io(String),

    #[error("Runtime error: {0}")]
    Runtime(String),
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err.to_string())
    }
}
