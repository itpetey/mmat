# MMAT

MMAT, short for **Make Me A Thing**, is a Rust workspace for turning an open-ended software request into a structured, observable delivery flow. It provides the foundations for agent-style project work: role coordination, semantic events, memory, LLM integration, repository discovery, project scaffolding, and controlled process execution.

The project is currently a library workspace. It does not expose a command-line interface yet.

## Why This Exists

Software prompts are often ambiguous, stateful, and hard to audit. MMAT is intended to make that work explicit by modelling delivery as a set of roles, contracts, events, memories, and artefacts.

The core goals are:

- Convert broad intent into scoped tasks and role-specific responsibilities.
- Keep decisions, claims, evidence, task state, and artefacts visible through semantic events.
- Preserve useful context in typed memory with provenance, confidence, authority, and decay rules.
- Support deterministic coordination around budgets, escalation, lifecycle state, and completion criteria.
- Keep project operations isolated through repository discovery, worktree handling, scaffolding, and shell execution primitives.

## Workspace Layout

| Crate | Purpose |
| --- | --- |
| `mmat-coordinator` | Role runtime, contracts, budget management, escalation routing, scheduling, and retrieval planning. |
| `mmat-event-stream` | Semantic event types, publish-subscribe event bus, and Postgres-backed event persistence. Legacy SQLite support retained for migration. |
| `mmat-llm` | OpenAI-compatible chat completions, streaming responses, and tool execution support. |
| `mmat-memory` | Typed semantic memory built on event streams, Postgres storage, Qdrant vector search, attention, provenance, and librarian workflows. Legacy SQLite support retained for migration. |
| `mmat-migration` | SQLite-to-Postgres migration utility for events, memories, and artefact blobs. |
| `mmat-process` | Shell command execution with working-directory and environment support. |
| `mmat-project` | Repository discovery, project type detection, project scaffolding, git worktree handling, and related project operations. |
| `mmat-roles` | Built-in delivery roles including intent lead, scholar, architect, project manager, ops manager, reviewer, auditor, and worker. |
| `mmat-workbench` | Browser-based MVP frontend for intent conversation, live semantic events, role status, artefacts, and memory/evidence inspection. |

## Requirements

- Rust toolchain with Edition 2024 support.
- Cargo.
- Postgres 16 with pgvector for event, memory, and artefact storage.
- Optional: Qdrant for vector-backed memory experiments.
- Optional: an OpenAI-compatible API endpoint for `mmat-llm` integration.

Start the local Postgres service with:

```bash
docker compose up -d postgres
```

Set `DATABASE_URL` in your environment using the connection details from `.env.example`:

```bash
export DATABASE_URL=postgres://mmat:mmat@localhost:5432/mmat
```

## Usage

Add the relevant crate from this workspace to another Rust crate while MMAT is under active local development:

```toml
[dependencies]
mmat-event-stream = { path = "../memory/crates/event-stream" }
mmat-memory = { path = "../memory/crates/memory" }
mmat-coordinator = { path = "../memory/crates/coordinator" }
```

Use the event stream to publish and subscribe to semantic events:

```rust
use mmat_event_stream::{
    event::{RoleId, SemanticEvent},
    event_bus::EventBus,
};

#[tokio::main]
async fn main() {
    let bus = EventBus::new(64);
    let mut receiver = bus.subscribe(&[]);

    bus.publish(SemanticEvent::new_tool_executed(
        RoleId::new("worker"),
        "cargo test",
        "{}",
        0,
        "ok",
        "",
        42,
    ))
    .unwrap();

    let event = receiver.recv().await.unwrap();
    println!("received {}", event.variant_name());
}
```

Use the workspace directly during development:

```bash
cargo build
cargo test
```

Run the prototype workbench frontend:

```bash
DATABASE_URL=postgres://mmat:mmat@localhost:5432/mmat cargo run -p mmat-workbench
```

Then open `http://127.0.0.1:8080`. Override the bind address with `MMAT_WORKBENCH_ADDR`, for example:

```bash
DATABASE_URL=postgres://mmat:mmat@localhost:5432/mmat \
  MMAT_WORKBENCH_ADDR=127.0.0.1:8090 cargo run -p mmat-workbench
```

The workbench requires a valid Postgres `DATABASE_URL` and will fail at startup with a clear message if one is not set. It hydrates its UI projection by replaying persisted Postgres events, so the browser resumes the previous conversation history instead of starting from a blank projection.

Migrate legacy SQLite stores into Postgres (one-off for existing `.mmat` data):

```bash
cargo run -p mmat-migration -- \
  --database-url postgres://mmat:mmat@localhost:5432/mmat \
  --sqlite-events events.db \
  --sqlite-memory memory.db \
  --artefacts-dir .mmat/artefacts
```

Add `--dry-run` to count events, memories, and artefact files without writing to Postgres.

## Development

Format, lint, and test before committing changes:

```bash
cargo fmt --all
cargo clippy -- -D warnings
cargo test
```

Build an optimised release artefact with:

```bash
cargo build --release
```

## Design Notes

- The workspace uses Rust Edition 2024.
- Dependencies are centralised in `[workspace.dependencies]` in the root `Cargo.toml`.
- The event stream is the main integration surface between roles, memory, and coordination.
- The workbench requires `DATABASE_URL` and uses Postgres-backed event, memory, and artefact stores exclusively. The coordinator runtime still supports SQLite and `.mmat/artefacts/` file-based fallback outside the workbench for backward compatibility; these legacy paths are retained for the migration tool and for non-workbench usage.
- `**/.mmat/` remains ignored for any remaining legacy data that has not yet been migrated.
- Memory entries carry metadata such as type, scope, authority, confidence, source role, evidence references, supersession, and decay policy.
- LLM support is provider-shaped around OpenAI-compatible chat completions rather than hard-wiring higher-level role behaviour to one service.
- Project operations are split into focused crates so orchestration code can remain separate from filesystem, process, and repository concerns.

## Status

MMAT is early-stage infrastructure. APIs are expected to change as the role model, memory lifecycle, and orchestration flow mature.

## Licence

This repository is distributed under the terms in [`LICENCE`](LICENCE).
