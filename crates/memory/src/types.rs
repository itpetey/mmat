use std::fmt;

use chrono::{DateTime, Utc};
use event_stream::event::{EventId, RoleId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MemoryId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MemoryType {
    Fact,
    Decision,
    Constraint,
    Preference,
    Risk,
    Lesson,
    SOP,
    Incident,
    Assumption,
    OpenQuestion,
    Relationship,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MemoryScope {
    Ephemeral,
    Project,
    Organisational,
    WorldModel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Authority {
    CompilerOutput,
    UserInstruction,
    RepositoryState,
    AcceptedADR,
    ReviewFindings,
    LLMInference,
    SpeculativeReasoning,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Confidence(f64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecayPolicy {
    Never,
    StaleAfterDays(u32),
    SupersededOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: MemoryId,
    pub memory_type: MemoryType,
    pub content: String,
    #[serde(skip)]
    pub embedding: Option<Vec<f32>>,
    pub scope: MemoryScope,
    pub authority: Authority,
    pub confidence: Confidence,
    pub decay_policy: DecayPolicy,
    pub evidence_refs: Vec<EventId>,
    pub supersedes: Option<MemoryId>,
    pub superseded_by: Option<MemoryId>,
    pub created_at: DateTime<Utc>,
    pub last_accessed_at: DateTime<Utc>,
    pub source_agent: RoleId,
}

#[derive(Default)]
pub struct MemoryBuilder {
    id: Option<MemoryId>,
    memory_type: Option<MemoryType>,
    content: Option<String>,
    embedding: Option<Vec<f32>>,
    scope: Option<MemoryScope>,
    authority: Option<Authority>,
    confidence: Option<Confidence>,
    decay_policy: Option<DecayPolicy>,
    evidence_refs: Vec<EventId>,
    supersedes: Option<MemoryId>,
    superseded_by: Option<MemoryId>,
    source_agent: Option<RoleId>,
}

impl MemoryId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for MemoryId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for MemoryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<Uuid> for MemoryId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl MemoryType {
    pub fn discriminant_str(&self) -> &'static str {
        match self {
            Self::Fact => "Fact",
            Self::Decision => "Decision",
            Self::Constraint => "Constraint",
            Self::Preference => "Preference",
            Self::Risk => "Risk",
            Self::Lesson => "Lesson",
            Self::SOP => "SOP",
            Self::Incident => "Incident",
            Self::Assumption => "Assumption",
            Self::OpenQuestion => "OpenQuestion",
            Self::Relationship => "Relationship",
        }
    }
}

impl TryFrom<&str> for MemoryType {
    type Error = crate::error::Error;

    fn try_from(s: &str) -> Result<Self> {
        match s {
            "Fact" => Ok(Self::Fact),
            "Decision" => Ok(Self::Decision),
            "Constraint" => Ok(Self::Constraint),
            "Preference" => Ok(Self::Preference),
            "Risk" => Ok(Self::Risk),
            "Lesson" => Ok(Self::Lesson),
            "SOP" => Ok(Self::SOP),
            "Incident" => Ok(Self::Incident),
            "Assumption" => Ok(Self::Assumption),
            "OpenQuestion" => Ok(Self::OpenQuestion),
            "Relationship" => Ok(Self::Relationship),
            _ => Err(crate::error::Error::InvalidMemoryType(s.to_string())),
        }
    }
}

impl MemoryScope {
    pub fn discriminant_str(&self) -> &'static str {
        match self {
            Self::Ephemeral => "Ephemeral",
            Self::Project => "Project",
            Self::Organisational => "Organisational",
            Self::WorldModel => "WorldModel",
        }
    }

    pub fn default_decay(&self) -> DecayPolicy {
        match self {
            Self::Ephemeral => DecayPolicy::StaleAfterDays(1),
            Self::Project => DecayPolicy::StaleAfterDays(365),
            Self::Organisational => DecayPolicy::StaleAfterDays(730),
            Self::WorldModel => DecayPolicy::Never,
        }
    }
}

impl TryFrom<&str> for MemoryScope {
    type Error = crate::error::Error;

    fn try_from(s: &str) -> Result<Self> {
        match s {
            "Ephemeral" => Ok(Self::Ephemeral),
            "Project" => Ok(Self::Project),
            "Organisational" => Ok(Self::Organisational),
            "WorldModel" => Ok(Self::WorldModel),
            _ => Err(crate::error::Error::InvalidMemoryScope(s.to_string())),
        }
    }
}

impl Authority {
    pub fn rank(&self) -> u8 {
        match self {
            Self::CompilerOutput => 7,
            Self::UserInstruction => 6,
            Self::RepositoryState => 5,
            Self::AcceptedADR => 4,
            Self::ReviewFindings => 3,
            Self::LLMInference => 2,
            Self::SpeculativeReasoning => 1,
        }
    }
}

impl Ord for Authority {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.rank().cmp(&other.rank())
    }
}

impl PartialOrd for Authority {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl TryFrom<&str> for Authority {
    type Error = crate::error::Error;

    fn try_from(s: &str) -> Result<Self> {
        match s {
            "CompilerOutput" => Ok(Self::CompilerOutput),
            "UserInstruction" => Ok(Self::UserInstruction),
            "RepositoryState" => Ok(Self::RepositoryState),
            "AcceptedADR" => Ok(Self::AcceptedADR),
            "ReviewFindings" => Ok(Self::ReviewFindings),
            "LLMInference" => Ok(Self::LLMInference),
            "SpeculativeReasoning" => Ok(Self::SpeculativeReasoning),
            _ => Err(crate::error::Error::InvalidAuthority(s.to_string())),
        }
    }
}

impl Confidence {
    pub fn new(value: f64) -> Result<Self> {
        if (0.0..=1.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err(crate::error::Error::InvalidConfidence(value))
        }
    }

    pub fn value(&self) -> f64 {
        self.0
    }
}

impl Default for Confidence {
    fn default() -> Self {
        Self(0.5)
    }
}

impl DecayPolicy {
    pub fn is_decayed(&self, created_at: DateTime<Utc>) -> bool {
        match self {
            Self::Never => false,
            Self::StaleAfterDays(days) => {
                let now = Utc::now();
                let threshold = created_at + chrono::Duration::days(*days as i64);
                now >= threshold
            }
            Self::SupersededOnly => false,
        }
    }

    pub fn discriminant_str(&self) -> String {
        match self {
            Self::Never => "Never".to_string(),
            Self::StaleAfterDays(days) => format!("StaleAfterDays({})", days),
            Self::SupersededOnly => "SupersededOnly".to_string(),
        }
    }
}

impl TryFrom<&str> for DecayPolicy {
    type Error = crate::error::Error;

    fn try_from(s: &str) -> Result<Self> {
        match s {
            "Never" => Ok(Self::Never),
            "SupersededOnly" => Ok(Self::SupersededOnly),
            _ if s.starts_with("StaleAfterDays(") => {
                let inner = s
                    .trim_start_matches("StaleAfterDays(")
                    .trim_end_matches(')');
                let days: u32 = inner
                    .parse()
                    .map_err(|_| crate::error::Error::InvalidDecayPolicy(s.to_string()))?;
                Ok(Self::StaleAfterDays(days))
            }
            _ => Err(crate::error::Error::InvalidDecayPolicy(s.to_string())),
        }
    }
}

impl Memory {
    pub fn builder() -> MemoryBuilder {
        MemoryBuilder::default()
    }
}

impl MemoryBuilder {
    pub fn id(mut self, id: MemoryId) -> Self {
        self.id = Some(id);
        self
    }

    pub fn memory_type(mut self, memory_type: MemoryType) -> Self {
        self.memory_type = Some(memory_type);
        self
    }

    pub fn content(mut self, content: impl Into<String>) -> Self {
        self.content = Some(content.into());
        self
    }

    pub fn embedding(mut self, embedding: Vec<f32>) -> Self {
        self.embedding = Some(embedding);
        self
    }

    pub fn scope(mut self, scope: MemoryScope) -> Self {
        self.scope = Some(scope);
        self
    }

    pub fn authority(mut self, authority: Authority) -> Self {
        self.authority = Some(authority);
        self
    }

    pub fn confidence(mut self, confidence: Confidence) -> Self {
        self.confidence = Some(confidence);
        self
    }

    pub fn decay_policy(mut self, decay_policy: DecayPolicy) -> Self {
        self.decay_policy = Some(decay_policy);
        self
    }

    pub fn evidence_refs(mut self, refs: Vec<EventId>) -> Self {
        self.evidence_refs = refs;
        self
    }

    pub fn supersedes(mut self, id: MemoryId) -> Self {
        self.supersedes = Some(id);
        self
    }

    pub fn superseded_by(mut self, id: MemoryId) -> Self {
        self.superseded_by = Some(id);
        self
    }

    pub fn source_agent(mut self, agent: RoleId) -> Self {
        self.source_agent = Some(agent);
        self
    }

    pub fn build(self) -> Result<Memory> {
        let memory_type = self
            .memory_type
            .ok_or_else(|| crate::error::Error::BuildError("memory_type is required".into()))?;
        let content = self
            .content
            .ok_or_else(|| crate::error::Error::BuildError("content is required".into()))?;
        let scope = self
            .scope
            .ok_or_else(|| crate::error::Error::BuildError("scope is required".into()))?;
        let authority = self
            .authority
            .ok_or_else(|| crate::error::Error::BuildError("authority is required".into()))?;
        let confidence = self
            .confidence
            .ok_or_else(|| crate::error::Error::BuildError("confidence is required".into()))?;
        let decay_policy = self.decay_policy.unwrap_or(scope.default_decay());
        let source_agent = self
            .source_agent
            .ok_or_else(|| crate::error::Error::BuildError("source_agent is required".into()))?;

        let now = Utc::now();
        Ok(Memory {
            id: self.id.unwrap_or_default(),
            memory_type,
            content,
            embedding: self.embedding,
            scope,
            authority,
            confidence,
            decay_policy,
            evidence_refs: self.evidence_refs,
            supersedes: self.supersedes,
            superseded_by: self.superseded_by,
            created_at: now,
            last_accessed_at: now,
            source_agent,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_id_new_and_display() {
        let id = MemoryId::new();
        let display = format!("{}", id);
        assert_eq!(display.len(), 36);
    }

    #[test]
    fn memory_id_from_uuid() {
        let uuid = Uuid::new_v4();
        let id = MemoryId::from(uuid);
        assert_eq!(id.0, uuid);
    }

    #[test]
    fn memory_type_discriminant_str() {
        assert_eq!(MemoryType::Fact.discriminant_str(), "Fact");
        assert_eq!(MemoryType::Decision.discriminant_str(), "Decision");
        assert_eq!(MemoryType::SOP.discriminant_str(), "SOP");
    }

    #[test]
    fn memory_type_try_from_str() {
        assert_eq!(MemoryType::try_from("Fact").unwrap(), MemoryType::Fact);
        assert!(MemoryType::try_from("Invalid").is_err());
    }

    #[test]
    fn memory_scope_default_decay() {
        assert!(matches!(
            MemoryScope::Ephemeral.default_decay(),
            DecayPolicy::StaleAfterDays(1)
        ));
        assert!(matches!(
            MemoryScope::WorldModel.default_decay(),
            DecayPolicy::Never
        ));
    }

    #[test]
    fn authority_ordering() {
        assert!(Authority::CompilerOutput > Authority::UserInstruction);
        assert!(Authority::UserInstruction > Authority::LLMInference);
        assert!(Authority::LLMInference > Authority::SpeculativeReasoning);
    }

    #[test]
    fn confidence_valid() {
        let c = Confidence::new(0.94).unwrap();
        assert!((c.value() - 0.94).abs() < f64::EPSILON);
    }

    #[test]
    fn confidence_invalid_high() {
        assert!(Confidence::new(1.5).is_err());
    }

    #[test]
    fn confidence_invalid_low() {
        assert!(Confidence::new(-0.1).is_err());
    }

    #[test]
    fn confidence_boundary_values() {
        assert!(Confidence::new(0.0).is_ok());
        assert!(Confidence::new(1.0).is_ok());
    }

    #[test]
    fn decay_policy_never_not_decayed() {
        let policy = DecayPolicy::Never;
        assert!(!policy.is_decayed(Utc::now() - chrono::Duration::days(100)));
    }

    #[test]
    fn decay_policy_stale_decayed() {
        let policy = DecayPolicy::StaleAfterDays(30);
        let created = Utc::now() - chrono::Duration::days(31);
        assert!(policy.is_decayed(created));
    }

    #[test]
    fn decay_policy_stale_not_decayed() {
        let policy = DecayPolicy::StaleAfterDays(30);
        let created = Utc::now() - chrono::Duration::days(15);
        assert!(!policy.is_decayed(created));
    }

    #[test]
    fn memory_builder_required_fields() {
        let result = Memory::builder().build();
        assert!(result.is_err());
    }

    #[test]
    fn memory_builder_success() {
        let memory = Memory::builder()
            .memory_type(MemoryType::Fact)
            .content("Test fact")
            .scope(MemoryScope::Project)
            .authority(Authority::UserInstruction)
            .confidence(Confidence::new(0.9).unwrap())
            .source_agent(RoleId::new("test"))
            .build()
            .unwrap();
        assert_eq!(memory.content, "Test fact");
        assert!(matches!(
            memory.decay_policy,
            DecayPolicy::StaleAfterDays(365)
        ));
    }

    #[test]
    fn memory_builder_with_explicit_decay() {
        let memory = Memory::builder()
            .memory_type(MemoryType::Fact)
            .content("Test")
            .scope(MemoryScope::Ephemeral)
            .authority(Authority::UserInstruction)
            .confidence(Confidence::new(0.5).unwrap())
            .decay_policy(DecayPolicy::Never)
            .source_agent(RoleId::new("test"))
            .build()
            .unwrap();
        assert!(matches!(memory.decay_policy, DecayPolicy::Never));
    }

    #[test]
    fn memory_serialization_skips_embedding() {
        let memory = Memory::builder()
            .memory_type(MemoryType::Fact)
            .content("Test")
            .scope(MemoryScope::Project)
            .authority(Authority::UserInstruction)
            .confidence(Confidence::new(0.5).unwrap())
            .embedding(vec![0.1, 0.2, 0.3])
            .source_agent(RoleId::new("test"))
            .build()
            .unwrap();
        let json = serde_json::to_string(&memory).unwrap();
        assert!(!json.contains("embedding"));
    }
}
