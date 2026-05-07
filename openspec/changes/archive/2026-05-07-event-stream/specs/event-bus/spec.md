## ADDED Requirements

### Requirement: Event bus supports topic-based subscription
The system SHALL allow subscribers to register interest in a subset of `SemanticEvent` variants. Subscribers MUST receive only events matching their registered variants. Registration MUST occur at subscription time via a filter function or variant set.

#### Scenario: Subscriber registers for specific variants
- **WHEN** a subscriber calls `bus.subscribe(&[EventType::TaskAssigned, EventType::ReviewCompleted])`
- **THEN** the returned receiver MUST yield only events whose variant matches one of the registered types
- **AND** events of other types published to the bus MUST be silently skipped for this subscriber

#### Scenario: Multiple independent subscribers with different filters
- **WHEN** Subscriber A registers for `[TaskAssigned]` and Subscriber B registers for `[ReviewCompleted]`
- **THEN** publishing `TaskAssigned` MUST be received by A but not B
- **AND** publishing `ReviewCompleted` MUST be received by B but not A
- **AND** publishing `DecisionRecorded` MUST be received by neither

### Requirement: Event bus provides backpressure awareness
The system SHALL report when a subscriber has fallen behind the broadcast buffer. The lagged count MUST indicate how many events were missed. The bus MUST NOT block publishers due to slow subscribers.

#### Scenario: Subscriber detects missed events after lag
- **WHEN** a subscriber processes events slower than the publish rate
- **AND** the broadcast buffer overflows for that subscriber's channel
- **THEN** the subscriber's next `recv()` MUST return `RecvError::Lagged(n)` where `n > 0`
- **AND** `n` MUST indicate the number of events that were dropped for this subscriber

#### Scenario: Publisher is never blocked by slow subscribers
- **WHEN** a publisher calls `bus.publish(event)` and one subscriber is processing a previous event slowly
- **THEN** the publish call MUST return immediately without awaiting the slow subscriber
