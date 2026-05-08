## ADDED Requirements

### Requirement: Artefact blobs are stored as JSONB in Postgres
The system SHALL provide an `artefacts` table in Postgres to store artefact payloads. Each row MUST contain a UUID artefact ID (primary key), artefact type discriminator, content hash (FNV-1a 64-bit), JSONB payload, producer role, and created-at timestamp. The payload column SHALL use `jsonb` for queryability and TOAST for large blobs.

#### Scenario: Artefact is stored and retrievable
- **WHEN** `store_artefact(artefact_type, payload)` is called
- **THEN** an artefact row is inserted with a new UUID artefact_id
- **AND** `get_artefact(artefact_id)` returns the full artefact with matching payload

#### Scenario: Artefact payload is queryable via JSONB
- **WHEN** artefacts of type `"adr"` exist with payloads containing different `"status"` values
- **THEN** querying by `payload->>'status' = 'approved'` MUST return only matching artefacts

### Requirement: Artefact retrieval replaces filesystem reads
The system SHALL replace all `std::fs::read_to_string` calls on `.mmat/artefacts/` files with `SELECT payload FROM artefacts WHERE artefact_id = $1`. The `storage_uri` format SHALL change from `file://` to `db://artefacts/{artefact_id}` for the new backend, with backward-compatible fallback to `file://` for existing rows.

#### Scenario: Artefact retrieved by storage URI
- **WHEN** `read_artefact_payload("db://artefacts/<uuid>")` is called
- **THEN** the payload is fetched from the Postgres `artefacts` table by artefact_id
- **AND** the result matches the originally stored payload

### Requirement: Artefact storage is transactional with event publishing
When an `ArtefactProduced` event is published, the artefact blob SHALL be inserted in the same Postgres transaction as the event row, ensuring no orphan blobs or dangling references.

#### Scenario: Artefact and event committed atomically
- **WHEN** a role calls `store_and_publish_artefact()` to store a blob and emit an `ArtefactProduced` event
- **THEN** both the artefact row and the event row MUST be visible together or neither visible (atomic commit)

#### Scenario: Rollback on failure
- **WHEN** the artefact INSERT succeeds but the event INSERT fails
- **THEN** the artefact row MUST NOT be visible (rolled back)
