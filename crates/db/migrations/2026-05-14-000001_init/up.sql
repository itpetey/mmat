CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE IF NOT EXISTS events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    rowid BIGSERIAL NOT NULL,
    variant TEXT NOT NULL,
    payload JSONB NOT NULL,
    timestamp_ns BIGINT NOT NULL,
    source_agent TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_events_rowid ON events(rowid);
CREATE INDEX IF NOT EXISTS idx_events_variant ON events(variant);

CREATE TABLE IF NOT EXISTS memories (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    memory_type TEXT NOT NULL,
    content TEXT NOT NULL,
    scope TEXT NOT NULL,
    authority TEXT NOT NULL,
    confidence DOUBLE PRECISION NOT NULL,
    decay_policy TEXT NOT NULL,
    evidence_refs TEXT NOT NULL DEFAULT '[]',
    supersedes UUID,
    superseded_by UUID,
    created_at TEXT NOT NULL,
    last_accessed_at TEXT NOT NULL,
    source_agent TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_memories_type ON memories(memory_type);
CREATE INDEX IF NOT EXISTS idx_memories_scope ON memories(scope);
CREATE INDEX IF NOT EXISTS idx_memories_authority ON memories(authority);
CREATE INDEX IF NOT EXISTS idx_memories_superseded_by ON memories(superseded_by);
CREATE INDEX IF NOT EXISTS idx_memories_decay ON memories(decay_policy, created_at);

CREATE TABLE IF NOT EXISTS artefacts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    artefact_type TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    payload JSONB NOT NULL,
    producer_role TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_artefacts_type ON artefacts(artefact_type);

CREATE TABLE IF NOT EXISTS projects (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    label VARCHAR NOT NULL,
    path VARCHAR NOT NULL
);

CREATE TABLE IF NOT EXISTS lanes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id TEXT NOT NULL,
    title TEXT NOT NULL,
    summary TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL,
    creator TEXT NOT NULL,
    parent_lane_id TEXT NULL,
    origin_event_id UUID NULL,
    origin_message_id TEXT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    archived_at TEXT NULL
);

CREATE INDEX IF NOT EXISTS idx_lanes_project_status ON lanes(project_id, status);
