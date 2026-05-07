## ADDED Requirements

### Requirement: Event store persists events in SQLite
The system SHALL store every published event in a SQLite database. The `events` table MUST have columns for a UUID event ID (primary key), a monotonically increasing `rowid` for efficient range scans, variant discriminator, full JSON payload, nanosecond timestamp, and source agent identifier. The store MUST open the database in WAL journal mode.

#### Scenario: Event store creates schema on first open
- **WHEN** `EventStore::open(path)` is called on a new database
- **THEN** the `events` table and variant index MUST be created automatically
- **AND** the database MUST use WAL journal mode

#### Scenario: Event store reopens existing database
- **WHEN** `EventStore::open(path)` is called on a database that already contains events
- **THEN** the existing events MUST be readable via query methods
- **AND** new events MUST be appended after the existing max `event_id`

### Requirement: Events can be replayed by row range
The system SHALL provide a method to retrieve events within a row range using the monotonically increasing `rowid`, from `after` (exclusive) to `before` (inclusive). Results MUST be returned in ascending `rowid` order (insertion order).

#### Scenario: Replay from a known point
- **WHEN** a subscriber calls `store.replay(after_row: 42, before_row: 100)`
- **THEN** it MUST receive events with `rowid > 42 AND rowid <= 100`
- **AND** events MUST be ordered by `rowid ASC`

#### Scenario: Replay with no upper bound
- **WHEN** a subscriber calls `store.replay(after_row: 42, before_row: None)`
- **THEN** it MUST receive all events with `rowid > 42`
- **AND** the result set MUST include the most recently stored event

### Requirement: Events can be queried by variant type
The system SHALL provide a method to retrieve events of a specific variant within an optional row range.

#### Scenario: Query for all TaskAssigned events
- **WHEN** a caller queries for variant `"TaskAssigned"` with no row range
- **THEN** all events with that variant discriminant MUST be returned
- **AND** events of other variants MUST NOT be included

#### Scenario: Query for variant within a row window
- **WHEN** a caller queries for variant `"ToolExecuted"` between row 100 and 200
- **THEN** only matching events within that row range MUST be returned

### Requirement: Most recent row in the store is queryable
The system SHALL provide a method to retrieve the maximum `rowid` in the store, or `None` if the store is empty.

#### Scenario: Get latest row from populated store
- **WHEN** the store contains 3 events
- **THEN** `store.latest_row()` MUST return `Some(3)`

#### Scenario: Get latest row from empty store
- **WHEN** the store contains no events
- **THEN** `store.latest_row()` MUST return `None`
