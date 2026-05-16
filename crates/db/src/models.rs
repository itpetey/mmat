use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::events)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Event {
    pub id: Uuid,
    pub rowid: i64,
    pub variant: String,
    pub payload: Value,
    pub timestamp_ns: i64,
    pub source_agent: String,
}

#[derive(Debug, Clone, Insertable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::events)]
pub struct NewEvent {
    pub id: Uuid,
    pub variant: String,
    pub payload: Value,
    pub timestamp_ns: i64,
    pub source_agent: String,
}

#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::lanes)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Lane {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub summary: String,
    pub status: String,
    pub creator: String,
    pub parent_lane_id: Option<String>,
    pub origin_event_id: Option<Uuid>,
    pub origin_message_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
}

#[derive(Debug, Clone, Insertable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::lanes)]
pub struct NewLane {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub summary: String,
    pub status: String,
    pub creator: String,
    pub parent_lane_id: Option<String>,
    pub origin_event_id: Option<Uuid>,
    pub origin_message_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
}

#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::memories)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Memory {
    pub id: Uuid,
    pub memory_type: String,
    pub content: String,
    pub scope: String,
    pub authority: String,
    pub confidence: f64,
    pub decay_policy: String,
    pub evidence_refs: String,
    pub supersedes: Option<Uuid>,
    pub superseded_by: Option<Uuid>,
    pub created_at: String,
    pub last_accessed_at: String,
    pub source_agent: String,
}

#[derive(Debug, Clone, Insertable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::memories)]
pub struct NewMemory {
    pub memory_type: String,
    pub content: String,
    pub scope: String,
    pub authority: String,
    pub confidence: f64,
    pub decay_policy: String,
    pub evidence_refs: String,
    pub supersedes: Option<Uuid>,
    pub superseded_by: Option<Uuid>,
    pub created_at: String,
    pub last_accessed_at: String,
    pub source_agent: String,
}

#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::artefacts)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Artefact {
    pub id: Uuid,
    pub artefact_type: String,
    pub content_hash: String,
    pub payload: Value,
    pub producer_role: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Insertable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::artefacts)]
pub struct NewArtefact {
    pub artefact_type: String,
    pub content_hash: String,
    pub payload: Value,
    pub producer_role: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::projects)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Project {
    pub id: Uuid,
    pub label: String,
    pub path: String,
}

#[derive(Debug, Clone, Insertable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::projects)]
pub struct NewProject {
    pub label: String,
    pub path: String,
}
