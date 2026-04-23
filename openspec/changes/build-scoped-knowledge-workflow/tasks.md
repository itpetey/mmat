## 1. Workflow foundation

- [x] 1.1 Add the direct NAAF dependencies required for knowledge orchestration and SQLite-backed persistence in `Cargo.toml`.
- [x] 1.2 Replace the current stub workflow entrypoints with a subject-oriented `src/workflow/` module layout that groups code by stage rather than by artefact type.
- [x] 1.3 Introduce shared runtime interfaces for live human questions and stage-scoped knowledge access without coupling workflow modules to the browser UI implementation.

## 2. Discovery workflow

- [x] 2.1 Implement the discovery stage module with its local types, prompt construction, and per-turn NAAF step builder.
- [x] 2.2 Implement live recursive discovery orchestration that re-enters discovery with accumulated answers until the result is ready for solution generation.
- [x] 2.3 Add tests covering live clarification handling, reuse of prior answers, and emission of a structured solution-ready discovery handoff.

## 3. Scoped knowledge planning and materialisation

- [x] 3.1 Define the `KnowledgePlan`, controlled knowledge-group template vocabulary, and run-scoped group instance model.
- [x] 3.2 Implement knowledge planning as a separate stage that proposes zero or more candidate groups, source types, and downstream consumers.
- [x] 3.3 Implement deterministic knowledge materialisation using `SqliteKnowledgeGroupStore`, backing-store initialisation, and ingestion of the available source types supported in the first pass.
- [x] 3.4 Implement stage-scoped knowledge session construction so each workflow stage receives only its selected materialised groups.
- [x] 3.5 Record the required upstream NAAF follow-up work for richer knowledge acquisition and duplicate detection.

## 4. Solution branch generation and user selection

- [x] 4.1 Implement concurrent conservative, recommended, and ambitious solution branch generation from the discovery handoff and selected knowledge groups.
- [x] 4.2 Implement the collect step that compares branches, recommends one branch or a hybrid, and records the recommendation rationale.
- [x] 4.3 Implement the live user choice step that accepts branch selection, hybrid selection, or revision feedback and routes revisions back to the appropriate earlier stage.
- [x] 4.4 Add tests covering distinct branch generation, hybrid recommendation, and user-driven revision routing.

## 5. Software Architect handoff

- [x] 5.1 Define the handoff contract from solution selection into a dedicated Software Architect stage.
- [x] 5.2 Implement the architect stage module so it consumes the selected solution and architect-scoped knowledge groups and produces planning-ready output.
- [x] 5.3 Connect the architect output to the next planning/execution boundary without reintroducing the old module-by-artefact structure.

## 6. Validation and repo documentation

- [x] 6.1 Update `README.md` to describe the rewritten workflow shape and the stage-specific knowledge model.
- [x] 6.2 Run `cargo fmt --all`, `cargo clippy -- -D warnings`, and `cargo test`, fixing any failures introduced by the new workflow code.
- [x] 6.3 Validate the OpenSpec change and confirm the change is ready for implementation/application.
