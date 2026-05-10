## Context

The workbench currently has a few focused unit tests and manual smoke checks. As it gains persistence, lanes, actions, and runtime routing, regressions will be easy to miss without integration coverage.

## Goals / Non-Goals

**Goals:**
- Cover critical HTTP endpoints and SSE behaviour.
- Test projection replay and restart semantics.
- Test user-facing routing decisions like mentions and action requests.
- Keep tests deterministic and isolated.

**Non-Goals:**
- Full browser automation unless a frontend harness is introduced.
- Testing every visual style detail.

## Decisions

- Prefer Rust integration tests around the Axum app and projection functions.
- Use temporary Postgres schemas/databases where persistence is required.
- Keep manual browser checks documented for visual polish until a browser harness exists.

## Risks / Trade-offs

- Postgres tests can be slower or environment-sensitive. Mitigation: reuse existing optional Postgres test utilities.
- SSE tests can be flaky. Mitigation: use bounded streams and explicit timeouts.
