## Why

MMAT is being rewritten because the previous architecture made the workflow hard to follow and coupled core orchestration too tightly to the web frontend. The rewrite needs to preserve the high-level workflow shape while introducing explicit subject-oriented workflow modules, live recursive discovery, and narrowly scoped knowledge exposure so each stage sees only the evidence it needs.

## What Changes

- Replace the stub workflow with a subject-oriented workflow layout under `src/workflow/`, where each stage owns its own prompts, types, and step construction.
- Add a live human-in-the-loop discovery stage that can recursively ask follow-up questions until the gathered context is ready for downstream solution design.
- Introduce a separate knowledge planning and knowledge materialisation flow that proposes, persists, and materialises zero or more knowledge groups for downstream learning.
- Scope knowledge exposure per workflow stage so discovery, solution generation, architecture, and later stages each receive only the relevant knowledge groups.
- Generate conservative, recommended, and ambitious solution branches concurrently, then collect them into a recommendation step that presents the options, recommends one or a hybrid, and asks the user to choose.
- Hand the selected solution and scoped knowledge forward to a downstream Software Architect stage before implementation planning.
- Use SQLite-backed OpenSpec/NAAF persistence for knowledge-group metadata instead of filesystem-backed group storage.
- Plan upstream NAAF improvements where current knowledge ingestion or duplicate-detection capabilities are insufficient, rather than embedding MMAT-specific workarounds.

## Capabilities

### New Capabilities
- `discovery-workflow`: live recursive discovery that gathers intent, asks clarification questions, and produces architect-ready context.
- `scoped-knowledge-groups`: knowledge planning, SQLite-backed group persistence, materialisation, and step-scoped knowledge exposure.
- `solution-branch-selection`: concurrent conservative/recommended/ambitious solution generation with collect, recommendation, and user choice.

### Modified Capabilities

None.

## Impact

- Affected code: `src/workflow/`, `src/main.rs`, and the runtime/UI integration surface that hands live human answers into workflow steps.
- Dependencies: add and use `naaf-knowledge`, `naaf-qdrant`, and `naaf-persistence-sqlite` directly in the rewrite.
- Systems: OpenSpec becomes the planning contract for this rewrite; MMAT runtime orchestration must stay decoupled from the browser UI.
- Upstream impact: this change identifies required NAAF improvements for richer knowledge acquisition and duplicate detection.
