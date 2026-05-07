## Why

Before the organisation can design or build anything, it must understand what the human wants (Intent Lead), what is true about the problem domain and codebase (Scholar), and what standards and processes should govern the work (Ops Manager). These three roles form the upstream understanding layer — they produce the intent briefs, research briefs, evidence packs, SOPs, rubrics, and standards that the Architect, Project Manager, Worker, and Reviewer consume downstream. Without them, the organisation operates on hallucinated assumptions.

## What Changes

- **New: Intent Lead role** — Persistent actor that interrogates human intent, clarifies ambiguity, identifies unstated goals and non-goals, maintains a stakeholder model, tracks satisfaction criteria, and captures taste. Publishes `IntentBrief`, `Constraints`, and `SuccessMetrics` as artefacts. Dispatches Scholars and Ops Managers to gather specific knowledge. **Must NOT invent implementation ideas** — only captures what the human wants, not how to build it.

- **New: Scholar role** — Persistent actor that gathers evidence, studies repository state, researches prior art, studies documentation, identifies uncertainty, and tracks confidence levels. Publishes `ResearchBrief`, `EvidencePack`, `OpenQuestions`, `ConstraintDiscoveries`, and `RiskReport` artefacts. Subscribes to `HumanFeedbackReceived` to refine research direction. Has a configurable research budget (LLM calls, web searches, tool invocations). **Must NOT decide process or architecture** — only answers "what appears true?"

- **New: Ops Manager role** — Persistent actor that owns organisational quality systems: SOPs, coding standards, review rubrics, deployment standards, testing requirements, architectural policies, and project playbooks. Publishes `ProcessProfile`, `ValidationPolicy`, `ReviewRubric`, `EscalationRules`, and `DeliveryStandards` artefacts. Maintains a procedural memory system ("for backend migrations, use migration playbook v3"). Continuously self-improves via research budget.

## Capabilities

### New Capabilities

- `intent-lead`: Human interrogation, intent brief production, constraint capture, success metrics, stakeholder model, taste tracking, and Scholar/Ops dispatch. Implements the `Role` trait.
- `scholar`: Evidence gathering, repo study, prior art research, documentation study, uncertainty identification, confidence tracking, research brief production, and evidence pack assembly. Implements the `Role` trait.
- `ops-manager`: SOP library management, review rubric definition, validation policy authoring, delivery standard setting, escalation rule definition, procedural memory maintenance, and continuous process improvement. Implements the `Role` trait.

### Modified Capabilities

None — fourth changeset in greenfield workspace.

## Impact

- **New crate**: `crates/roles/` (or modules within the coordinator pattern — roles are actors, not a separate crate). Decision deferred to design.md.
- **Dependencies**: `event-stream` (event types, bus), `memory` (memory store for retrieval), `coordinator` (Role trait, RoleContext, CoordinatorHandle), `llm` (LlmClient, Executor, ToolRegistry), `process` (ProcessCommand for repo study)
- **Event stream**: Intent Lead publishes `HumanFeedbackRequested`; Scholar publishes `ClaimMade`, `MemoryProposed`; Ops Manager publishes `DecisionRecorded` (SOPs/rubrics as durable decisions)
- **Authority hierarchy**: Intent Lead (UserInstruction authority), Scholar (LLMInference → upgraded to ReviewFindings when evidence-backed), Ops Manager (AcceptedADR authority for SOPs)
