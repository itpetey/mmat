# Main improvements I’d recommend

## 1. Do not use `EventId` as `MemoryId`

I noticed `MemoryAccepted` carries `memory_id: EventId`, while the memory crate has its own `MemoryId`. That will become confusing and dangerous. Keep them distinct everywhere.

Use:

```rust
pub struct MemoryAccepted {
    memory_id: MemoryId,
    proposal_event_id: EventId,
}
```

## 2. Add project / organisation / run IDs to events

Right now events have source agent and timestamp, but not enough scoping. For an R&D-house architecture, every event should carry:

```
organisation_id
workspace_id / department_id
project_id
run_id
task_id optional
causation_id
correlation_id
```

Without this, cross-project memory will become polluted quickly.

## 3. Replace heuristic embeddings

The hash-style `compute_simple_embedding` is fine for tests, but contradiction/duplicate logic depends on it too much. Make embedding a trait and use fake embeddings in tests, real embeddings in production.

## 4. Make artefacts first-class storage, not giant event payloads

Worker currently stores implementation patches inside artefact references. That is okay for early tests, but event logs should point to artefact blobs, not contain large blobs.

Use:

```
ArtefactProduced {
  artefact_id,
  artefact_type,
  content_hash,
  storage_uri,
  producer_role,
  evidence_refs
}
```

## 5. Strengthen process policies

The Auditor currently has hardcoded process checks such as “tests passed” implies `cargo test`. That is a good seed, but this should come from Ops Manager SOPs.

Ideal flow:

```
Ops Manager publishes validation policy
Worker executes
Auditor checks against active policy
Reviewer checks human-quality dimensions
```

## 6. Make event delivery semantics explicit

Tokio broadcast is a good start, but consumers can lag. You already surface Lagged, but roles need recovery behaviour: replay from event store cursor, resume, then resubscribe.

That will matter once Attention, Auditor, Librarian, and PM are all live.
