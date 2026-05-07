## Why

A persistent engineering organisation that produces code and makes decisions without oversight is dangerous. The Auditor is the institutional sceptic — it watches every event, verifies every claim against evidence, detects when agents skip process steps, and surfaces unjustified confidence. Without an Auditor, hallucinated "facts" become durable memory, unverified "tests passed" claims become accepted truth, and the organisation drifts into an opaque, untrustworthy state. The Auditor is the governance role that makes the system auditable and falsifiable.

## What Changes

- **New: Auditor role** — An adversarial (systematically distrustful) persistent actor that monitors the event stream for: process adherence (did the Worker run tests before claiming they passed?), evidence integrity (does a claim's evidence actually exist in the event store?), hallucination detection (does a Scholar cite non-existent sources?), unjustified confidence (does a claim with no evidence assert high confidence?), policy compliance (did roles stay within their authority boundaries?), and memory contamination (did ungrounded claims become durable memories?).

- **New: Audit dimensions** — The Auditor inspects evidence chains, not raw outputs. For every `ClaimMade` event, it checks: (1) are evidence references valid (point to real events)? (2) is the claim consistent with the evidence? (3) was the required process followed? (4) were policies violated?

- **New: Audit reports** — The Auditor publishes `AuditReport` artefacts documenting findings, confidence assessments, policy violations, and traceability reports. Audit findings are themselves events — `PolicyViolationDetected`, `EvidenceChainBroken`, `ProcessSkipped`.

## Capabilities

### New Capabilities

- `auditor`: Event stream monitoring, evidence chain validation, process adherence checking, hallucination detection, confidence assessment, policy compliance verification, and audit report production. Implements the `Role` trait with `AuthorityScope::Audit`. The Auditor is adversarial — it actively searches for failures rather than passively processing events.

### Modified Capabilities

None — sixth and final changeset in greenfield workspace.

## Impact

- **Extends** `crates/roles/` with `auditor.rs` module
- **Dependencies**: event-stream (consumes all event types, uses EventStore for evidence lookups), memory (checks memory store for contamination), coordinator (reports violations), provenance-engine (traces evidence chains)
- **Event stream**: Subscribes to ALL event types — the Auditor has the broadest subscription in the organisation. Publishes `PolicyViolationDetected`, `EvidenceChainBroken`, `ProcessSkipped`, `AuditReport` events.
- **No LLM usage**: The Auditor primarily uses deterministic checks against the event store and provenance engine. An LLM may be used selectively for nuanced judgment (e.g., "is this claim semantically consistent with its cited evidence?") but most audit dimensions are rule-based.
