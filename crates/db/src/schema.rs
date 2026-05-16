diesel::table! {
    artefacts (id) {
        id -> Uuid,
        artefact_type -> Text,
        content_hash -> Text,
        payload -> Jsonb,
        producer_role -> Text,
        created_at -> Text,
    }
}

diesel::table! {
    events (id) {
        id -> Uuid,
        rowid -> Int8,
        variant -> Text,
        payload -> Jsonb,
        timestamp_ns -> Int8,
        source_agent -> Text,
    }
}

diesel::table! {
    lanes (id) {
        id -> Text,
        project_id -> Text,
        title -> Text,
        summary -> Text,
        status -> Text,
        creator -> Text,
        parent_lane_id -> Nullable<Text>,
        origin_event_id -> Nullable<Uuid>,
        origin_message_id -> Nullable<Text>,
        created_at -> Text,
        updated_at -> Text,
        archived_at -> Nullable<Text>,
    }
}

diesel::table! {
    memories (id) {
        id -> Uuid,
        memory_type -> Text,
        content -> Text,
        scope -> Text,
        authority -> Text,
        confidence -> Float8,
        decay_policy -> Text,
        evidence_refs -> Text,
        supersedes -> Nullable<Uuid>,
        superseded_by -> Nullable<Uuid>,
        created_at -> Text,
        last_accessed_at -> Text,
        source_agent -> Text,
    }
}

diesel::table! {
    projects (id) {
        id -> Uuid,
        label -> Varchar,
        path -> Varchar,
    }
}

diesel::allow_tables_to_appear_in_same_query!(artefacts, events, lanes, memories, projects,);
