//! Coordinator crate: deterministic role runtime with contract enforcement,
//! budget management, escalation routing, and lifecycle management.

pub use contract::{CompletionCriteria, Contract, ContractId, TaskContext};
pub use error::{Error, Result};
pub use registry::RoleRegistry;
pub use retrieval::{RetrievalPlanner, RetrievalProfile, default_profile_for_role_type};
pub use role::{
    AuthorityScope, Budget, CapabilityStatus, CoordinatorHandle, Role, RoleContext, RoleError,
    RoleLifecycleState, RoleReadiness, RoleSpec, RoleType, Severity, ToolRegistry,
};
pub use runtime::{OrganisationConfig, OrganisationRuntime};
pub use scheduler::{BudgetState, Scheduler};

pub mod contract;
pub mod error;
pub mod registry;
pub mod retrieval;
pub mod role;
pub mod runtime;
pub mod scheduler;
