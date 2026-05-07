## 1. Crate Setup

- [x] 1.1 Scaffold `crates/roles/Cargo.toml` with dependencies (event-stream, memory, coordinator, llm, process, serde, serde_json, tokio, uuid, thiserror, tracing, async-trait)
- [x] 1.2 Add `crates/roles` to workspace members in root `Cargo.toml`
- [x] 1.3 Create `src/lib.rs` with module declarations: `intent_lead`, `scholar`, `ops_manager`
- [x] 1.4 Define shared role artefact types in `src/artefacts.rs`: `IntentBrief`, `ResearchBrief`, `EvidencePack`, `OpenQuestions`, `ProcessProfile`, `ReviewRubric`, `ValidationPolicy`, `EscalationRules`, `DeliveryStandards`

## 2. Intent Lead

- [x] 2.1 Implement `IntentLead` struct with LLM client, tool registry (read tools only), memory store handle, coordinator handle
- [x] 2.2 Implement `Role` trait for `IntentLead`: define `id()`, `spec()` with AuthorityScope::IntentOnly, subscriptions to `HumanFeedbackReceived` and `TaskCompleted` (from dispatched roles)
- [x] 2.3 Implement `IntentLead::run()` actor loop: receive initial prompt, interrogate human with clarifying questions via `HumanFeedbackRequested`, iterate until confidence threshold met
- [x] 2.4 Implement intent brief assembly: collect goals, non-goals, constraints, success metrics, preferences, open questions into `IntentBrief` struct
- [x] 2.5 Publish `ArtefactProduced` event with serialised `IntentBrief`
- [x] 2.6 Implement Scholar/Ops dispatch: publish `TaskAssigned` events targeting Scholar or Ops Manager with research questions or process requirements
- [x] 2.7 Implement stakeholder model persistence: publish `MemoryProposed` events for stakeholder preferences
- [x] 2.8 Implement guard: parse output to detect implementation suggestions, reject and reprompt if found

## 3. Scholar

- [x] 3.1 Implement `Scholar` struct with LLM client, Executor, tool registry (read_file, web_search, knowledge_query, git_log), research budget config, memory store handle
- [x] 3.2 Implement `Role` trait for `Scholar`: define `id()`, `spec()` with AuthorityScope (can publish ClaimMade but NOT DecisionRecorded), subscriptions to `TaskAssigned`
- [x] 3.3 Implement `Scholar::run()` actor loop: receive `TaskAssigned` with research brief, execute tool-augmented LLM research loop, publish findings
- [x] 3.4 Implement repo study: use file reading tools to explore repository structure, identify patterns, read conventions
- [x] 3.5 Implement prior art research: use web search tools to find external techniques, cite sources
- [x] 3.6 Implement `ResearchBrief` assembly: summary of findings, key patterns, discovered constraints
- [x] 3.7 Implement `EvidencePack` assembly: per-finding claims with source references (file paths, URLs, event IDs), extracted content, confidence ratings
- [x] 3.8 Implement `OpenQuestions` identification: surface what was NOT found, uncertainties, suggested next research
- [x] 3.9 Publish `ArtefactProduced` events for each artefact type
- [x] 3.10 Publish `MemoryProposed` events for durable facts discovered (conventions, constraints, risks)
- [x] 3.11 Implement research budget tracking: count LLM calls, web searches, tool invocations; stop and escalate on exhaustion
- [x] 3.12 Implement guard: detect architectural recommendations in output, suppress and reprompt

## 4. Ops Manager

- [x] 4.1 Implement `OpsManager` struct with LLM client, Executor, tool registry, memory store handle, coordinator handle
- [x] 4.2 Implement `Role` trait for `OpsManager`: define `id()`, `spec()` with AuthorityScope (can publish DecisionRecorded for SOPs/rubrics), subscriptions to `TaskAssigned` and `ReviewCompleted`
- [x] 4.3 Implement `OpsManager::run()` actor loop: receive task assignments, produce SOPs/rubrics/policies, periodic review of existing SOPs
- [x] 4.4 Implement SOP creation: on task, generate step-by-step procedure with when-to-apply, preconditions, postconditions, rollback steps
- [x] 4.5 Implement `ReviewRubric` creation: define explicit review dimensions (correctness, API design, cohesion, coupling, backwards compat, observability, error handling, concurrency, performance, security, test adequacy, migration safety)
- [x] 4.6 Implement `ValidationPolicy` creation: specify tools to run, pass criteria, failure handling per project type (CLI, web service, embedded, proc macro)
- [x] 4.7 Implement `EscalationRules` creation: map failure classes (implementation defect, architectural conflict, missing knowledge, ambiguous intent, broken process) to escalation targets
- [x] 4.8 Implement `DeliveryStandards` creation: branch naming, commit message format, PR size limits, review requirements
- [x] 4.9 Publish SOPs/rubrics/policies as `MemoryProposed` events with type `SOP` and scope `Organisational`
- [x] 4.10 Implement periodic review loop (tokio::time::interval, default weekly): query for SOPs approaching decay, confirm or replace
- [x] 4.11 Implement continuous improvement: analyse `ReviewCompleted` events for recurring failures, propose rubric updates
- [x] 4.12 Implement external research: use web search to find current best practices, compare with existing SOPs, propose updates

## 5. Integration

- [x] 5.1 Write integration test: Intent Lead receives prompt → publishes clarifying question → human answers → IntentBrief published
- [x] 5.2 Write integration test: Intent Lead dispatches Scholar → Scholar researches → EvidencePack published → Intent Lead consumes findings
- [x] 5.3 Write integration test: Scholar researches repo → ResearchBrief published → findings stored as durable memories
- [x] 5.4 Write integration test: Scholar exceeds research budget → escalates → budget extension via new TaskAssigned
- [x] 5.5 Write integration test: Ops Manager creates SOP → stored as memory → retrieved by retrieval planner for relevant query
- [x] 5.6 Write integration test: Ops Manager periodic review detects stale SOP → proposes replacement
- [x] 5.7 Write integration test: Scholar output does not contain architectural decisions (guard check)
- [x] 5.8 Write integration test: Intent Lead output does not contain implementation suggestions (guard check)

## 6. Validation

- [x] 6.1 `cargo fmt --all` passes
- [x] 6.2 `cargo clippy -- -D warnings` passes on all crates
- [x] 6.3 `cargo test` passes all tests including doc tests
