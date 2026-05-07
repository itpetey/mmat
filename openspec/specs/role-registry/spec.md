# role-registry Specification

## Purpose
TBD - created by archiving change coordinator. Update Purpose after archive.
## Requirements
### Requirement: Role trait defines the actor contract
The system SHALL define a `Role` trait with methods for identity (`id()` returning `RoleId`), specification (`spec()` returning `RoleSpec`), event subscriptions (`subscriptions()` returning event types), and lifecycle (`run()` accepting `RoleContext` and returning a result). The trait MUST be `Send + Sync + 'static` and use `#[async_trait]`.

#### Scenario: Role is started by the coordinator
- **WHEN** the coordinator calls `role.run(ctx)` at startup
- **THEN** the role MUST begin processing events from its bus subscription
- **AND** the role MUST NOT return until it is stopped or fails

#### Scenario: Role publishes lifecycle events
- **WHEN** a role begins execution
- **THEN** it SHOULD publish a `TaskStarted` event (the coordinator also monitors for this)
- **WHEN** a role completes its current task
- **THEN** it MUST publish a `TaskCompleted` event with output artefacts

### Requirement: RoleSpec describes role capabilities and constraints
The system SHALL define a `RoleSpec` struct with fields: `role_type` (IntentLead, Scholar, OpsManager, Architect, ProjectManager, Worker, Reviewer, Auditor, Librarian), `authority_scope` (what decisions this role can make), `default_budget` (time and token limits), `input_contract` (what event types trigger this role), and `output_contract` (what event types this role produces).

#### Scenario: RoleSpec is used to register a role
- **WHEN** a role is registered with the coordinator
- **THEN** the `RoleSpec` MUST be stored in the role registry
- **AND** the coordinator MUST use the spec to validate that the role's published events match its output contract

#### Scenario: RoleSpec defines authority boundaries
- **WHEN** a role's `authority_scope` is set to `IntentOnly`
- **THEN** it MUST NOT publish `DecisionRecorded` events (that authority belongs to the Architect)

### Requirement: Role registry catalogues all roles in the organisation
The system SHALL provide a `RoleRegistry` that stores `RoleSpec`s by `RoleId`. The registry MUST support registration (with duplicate-ID rejection), lookup by ID, lookup by role type, and listing all registered roles. The registry MUST be populated at organisation startup before any role is run.

#### Scenario: Roles are registered before startup
- **WHEN** the coordinator boots
- **THEN** all role specs MUST be registered before the runtime starts dispatching events
- **AND** registering a role with a duplicate `RoleId` MUST return an error

#### Scenario: Escalation path is looked up from registry
- **WHEN** a role publishes `EscalationRequested` with severity `Moderate`
- **THEN** the scheduler MUST query the registry for the escalation target registered for that (role_id, severity) pair
- **AND** if no target is registered for that severity, the scheduler MUST escalate to the next higher severity's target

### Requirement: Role registry validates contract compatibility
The system SHALL validate that when role A has an escalation path to role B, role B's `input_contract` is compatible with role A's escalation event payload. Incompatible contracts MUST be rejected at registration time.

#### Scenario: Compatible contracts are accepted
- **WHEN** Worker's escalation path targets Reviewer at Moderate severity
- **AND** Reviewer's `input_contract` includes `ReviewRequested` events
- **THEN** the registration MUST succeed

#### Scenario: Incompatible contracts are rejected
- **WHEN** a role's escalation path targets a role that doesn't accept the escalation event type
- **THEN** registration MUST return an error explaining the mismatch

