## MODIFIED Requirements

### Requirement: Event store persists events in Postgres
The system SHALL store every published event in a Postgres database through `mmat-db` Diesel functions. The `events` table MUST have columns for a UUID event ID (primary key), a `BIGSERIAL rowid` for efficient range scans, variant discriminator, `JSONB` payload, nanosecond timestamp, and source agent identifier. Database dependencies and schema access MUST be confined to `mmat-db`; `mmat-event-stream` MUST NOT own SQLx, Rusqlite, Diesel, or database connection code.

#### Scenario: Event store uses mmat-db append function
- **WHEN** workbench or runtime code needs to persist a semantic event
- **THEN** it MUST call an `mmat-db` event append function
- **AND** the persisted row MUST include the event ID, variant, JSON payload, timestamp, and source agent

#### Scenario: Event store reconnects to existing database
- **WHEN** the application connects to a database with existing events
- **THEN** the existing events MUST be readable via `mmat-db` query methods
- **AND** new events MUST be appended after the existing max `rowid`

#### Scenario: Event store supports concurrent writers
- **WHEN** two services append events simultaneously through `mmat-db`
- **THEN** both events MUST persist with distinct `rowid` values
- **AND** both callers MUST receive `Ok`

### Requirement: Events can be replayed by row range
The system SHALL provide an `mmat-db` method to retrieve events within a row range using the `BIGSERIAL rowid`, from `after` (exclusive) to `before` (inclusive). Results MUST be returned in ascending `rowid` order (insertion order).

#### Scenario: Replay from a known point
- **WHEN** a subscriber calls the `mmat-db` replay function with `after_row: 42` and `before_row: 100`
- **THEN** it MUST receive events with `rowid > 42 AND rowid <= 100`
- **AND** events MUST be ordered by `rowid ASC`

#### Scenario: Replay with no upper bound
- **WHEN** a subscriber calls the `mmat-db` replay function with `after_row: 42` and no upper bound
- **THEN** it MUST receive all events with `rowid > 42`
- **AND** the result set MUST include the most recently stored event

### Requirement: Events can be queried by variant type
The system SHALL provide an `mmat-db` method to retrieve events of a specific variant within an optional row range.

#### Scenario: Query for all TaskAssigned events
- **WHEN** a caller queries for variant `'TaskAssigned'` with no row range
- **THEN** all events with that variant discriminant MUST be returned
- **AND** events of other variants MUST NOT be included

#### Scenario: Query for variant within a row window
- **WHEN** a caller queries for variant `'ToolExecuted'` between row 100 and 200
- **THEN** only matching events within that row range MUST be returned

### Requirement: Most recent row in the store is queryable
The system SHALL provide an `mmat-db` method to retrieve the maximum `rowid` in the store, or `NULL` if the store is empty.

#### Scenario: Get latest row from populated store
- **WHEN** the store contains 3 events
- **THEN** the latest-row query MUST return `Some(3)`

#### Scenario: Get latest row from empty store
- **WHEN** the store contains no events
- **THEN** the latest-row query MUST return `None`

## REMOVED Requirements

### Requirement: Event store creates schema on first open
**Reason**: Database schema ownership is moving to `mmat-db` and the project's Diesel/Postgres setup rather than runtime schema creation in `mmat-event-stream`.
**Migration**: Define schema and CRUD in `mmat-db`; application startup connects through `mmat-db` and does not call `EventStore::new(database_url)`.
