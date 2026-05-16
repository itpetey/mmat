## ADDED Requirements

### Requirement: Event bus is live-only
The event bus SHALL provide in-memory live pub/sub for `SemanticEvent` values and MUST NOT own durable database persistence. Callers that require durability MUST persist through `mmat-db` before broadcasting.

#### Scenario: Persisted event is broadcast
- **WHEN** a service successfully appends an event through `mmat-db`
- **THEN** it MAY broadcast the same event through `EventBus`
- **AND** subscribers MUST receive it according to their variant filters

#### Scenario: Event bus has no store attachment
- **WHEN** code constructs an `EventBus`
- **THEN** it MUST NOT require a database URL, SQLx pool, Rusqlite connection, or `EventStore`
