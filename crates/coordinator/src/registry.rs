//! Role registry for managing role specifications and event dispatch indexing.

use std::collections::HashMap;

use mmat_event_stream::event::{EventType, RoleId};

use crate::{
    error::{Error, Result},
    role::{RoleSpec, Severity},
};

/// Registry that holds all role specifications and indexes them for event dispatch.
#[derive(Clone)]
pub struct RoleRegistry {
    roles: HashMap<RoleId, RoleSpec>,
    dispatch_index: HashMap<EventType, Vec<RoleId>>,
}

impl RoleRegistry {
    /// Creates an empty role registry.
    pub fn new() -> Self {
        Self {
            roles: HashMap::new(),
            dispatch_index: HashMap::new(),
        }
    }

    /// Registers a role specification in the registry.
    ///
    /// Validates the input contract and escalation path compatibility
    /// before inserting.
    pub fn register(&mut self, spec: RoleSpec) -> Result<()> {
        let role_id = spec.id.clone();

        // Validate that input contract is a valid trigger event
        if !is_valid_trigger_event(&spec.input_contract) {
            return Err(Error::InvalidRoleSpec(format!(
                "input contract {:?} is not a valid trigger event",
                spec.input_contract
            )));
        }

        // Check for duplicate RoleId
        if self.roles.contains_key(&role_id) {
            return Err(Error::DuplicateRoleId(role_id.to_string()));
        }

        // Validate escalation path contract compatibility with scheduler routing:
        // escalation targets receive TaskAssigned events carrying escalation context.
        for (severity, target_id) in &spec.escalation_paths {
            if let Some(target_spec) = self.roles.get(target_id)
                && target_spec.input_contract != EventType::TaskAssigned
            {
                return Err(Error::InvalidRoleSpec(format!(
                    "escalation path {severity} -> {target_id} is incompatible: target does not accept TaskAssigned"
                )));
            }
        }

        // Also validate reverse: if any existing role escalates to this new role,
        // check that the new role accepts TaskAssigned.
        for (existing_id, existing_spec) in &self.roles {
            for target_id in existing_spec.escalation_paths.values() {
                if target_id == &role_id && spec.input_contract != EventType::TaskAssigned {
                    return Err(Error::InvalidRoleSpec(format!(
                        "existing role {existing_id} escalation to new role is incompatible: new role does not accept TaskAssigned"
                    )));
                }
            }
        }

        // Build dispatch index entry
        self.dispatch_index
            .entry(spec.input_contract.clone())
            .or_default()
            .push(role_id.clone());

        self.roles.insert(role_id, spec);
        Ok(())
    }

    /// Looks up a role specification by its ID.
    pub fn get(&self, id: RoleId) -> Option<&RoleSpec> {
        self.roles.get(&id)
    }

    /// Returns all role specifications matching a given [`RoleType`](crate::role::RoleType).
    pub fn get_by_type(&self, role_type: crate::role::RoleType) -> Vec<&RoleSpec> {
        self.roles
            .values()
            .filter(|spec| spec.role_type == role_type)
            .collect()
    }

    /// Returns the role specifications that subscribe to the given event type.
    pub fn subscribers_for(&self, event_type: &EventType) -> Vec<&RoleSpec> {
        self.dispatch_index
            .get(event_type)
            .map(|ids| ids.iter().filter_map(|id| self.roles.get(id)).collect())
            .unwrap_or_default()
    }

    /// Returns the configured escalation target `RoleId` for a given role and severity.
    /// Falls back to higher severities if no exact match is registered.
    pub fn escalation_target(&self, role_id: &RoleId, severity: &Severity) -> Option<RoleId> {
        let spec = self.roles.get(role_id)?;

        let severities = [
            Severity::Low,
            Severity::Medium,
            Severity::High,
            Severity::Critical,
        ];
        let start_idx = severities.iter().position(|s| s == severity).unwrap_or(0);

        for sev in &severities[start_idx..] {
            if let Some(target_id) = spec.escalation_paths.get(sev) {
                return Some(target_id.clone());
            }
        }
        None
    }

    /// Returns a reference to all registered roles.
    pub fn all_roles(&self) -> &HashMap<RoleId, RoleSpec> {
        &self.roles
    }
}

impl Default for RoleRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn is_valid_trigger_event(event_type: &EventType) -> bool {
    matches!(
        event_type,
        EventType::TaskAssigned
            | EventType::ReviewRequested
            | EventType::EscalationRequested
            | EventType::HumanFeedbackRequested
            | EventType::HumanFeedbackReceived
            | EventType::OrganisationStarted
    )
}
