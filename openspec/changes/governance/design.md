## Context

The Auditor is the organisation's institutional sceptic. Unlike other roles that are cooperative (trying to complete work), the Auditor is adversarial — it actively searches for failures in process adherence, evidence integrity, and policy compliance. It does not mutate memory or make decisions — it reports. The conversation design is explicit: "the Auditor should be different. Its job is distrust. Not maliciously, but systematically."

## Goals / Non-Goals

**Goals:**
- Monitor ALL event types on the event stream
- Validate evidence chains: do claims reference real tool executions?
- Detect process violations: were required steps skipped?
- Detect hallucinations: does evidence actually support claims?
- Assess confidence: do claims with weak evidence assert high confidence?
- Publish audit reports and policy violations

**Non-Goals:**
- Memory mutation — the Auditor reports, the Librarian acts
- LLM-driven judgment for routine checks — deterministic where possible
- Preventive enforcement — the Auditor detects post-hoc, doesn't block

## Decisions

### Decision 1: Auditor is primarily deterministic, selectively LLM-assisted

**Chosen**: Most audit dimensions use rule-based checks against the event store: does the referenced `EventId` exist? Was `cargo test` executed before `ClaimMade("tests passed")`? Does the claim's authority exceed the role's authority scope? An LLM is used only for nuanced semantic checks (e.g., "is this claim semantically consistent with its cited evidence?").

**Rationale**: Deterministic checks are fast, reliable, and auditable. LLM-based auditing would create infinite regress (who audits the Auditor?). The 80/20 rule: 80% of violations are structural (missing evidence, skipped steps, authority violations) and can be caught deterministically.

### Decision 2: Auditor inspects the event stream, not raw CoT

**Chosen**: The Auditor subscribes to all `SemanticEvent` types on the bus and inspects the structured event payloads and their relationships via the provenance engine. It does NOT inspect raw LLM chain-of-thought.

**Rationale**: This matches the conversation's three-layer cognition model: Layer 1 (raw CoT) is not audited, Layer 2 (semantic events) is the Auditor's primary input, Layer 3 (institutional knowledge) is governed by the Librarian.

### Decision 3: Auditor publishes findings, does not enforce

**Chosen**: When the Auditor detects a violation, it publishes a `PolicyViolationDetected` or `EvidenceChainBroken` event. It does NOT mutate memory, stop tasks, or override decisions. Enforcement is the Librarian's responsibility (for memory contamination) or the Reviewer's (for process violations).

**Rationale**: Checks and balances. The conversation is explicit: "the Librarian should NOT trust the Auditor absolutely. And the Auditor should NOT mutate memory directly." Separation of audit and enforcement prevents a single hallucinated audit from corrupting organisational truth.

### Decision 4: Auditor-to-Librarian feedback loop with human safety valve

**Chosen**: When the Auditor publishes `EvidenceChainBroken` or `PolicyViolationDetected` referencing a `MemoryId`, the Librarian automatically re-evaluates that memory. For memories with `authority >= UserInstruction`, the Librarian escalates to the human for confirmation before acting. Lower-authority memories are re-evaluated automatically.

**Rationale**: Creates the governance feedback loop the architecture intends, but prevents automated corruption of high-authority knowledge. A hallucinated audit finding against a `CompilerOutput`-authority memory triggers human review, not automatic reversal.

### Decision 5: Auditor human escalation for critical violations

**Chosen**: The Auditor publishes `HumanFeedbackRequested` via the Intent Lead when: a violation has `Critical` severity, or the same role produces the same violation type N times (configurable, default 3). Non-critical, non-repeated findings stay internal.

**Rationale**: The human is at the top of the authority hierarchy. Critical governance failures (e.g., systematic evidence fabrication, persistent authority violations) must be visible. Routine findings (e.g., a single unsubstantiated claim) are handled by the internal governance loop.

## Risks / Trade-offs

- **[Risk] Auditor produces false positives** → Mitigation: Violations include confidence ratings. The human or Librarian can dismiss low-confidence violations. Audit reports are themselves events subject to provenance.
- **[Risk] Auditor misses subtle violations** → Mitigation: The LLM-assisted path handles semantic inconsistencies. Audit dimensions are extensible — new check types can be added.
- **[Trade-off] Post-hoc detection means violations reach memory** → The Librarian is the memory write gate. The Auditor catches what the Librarian misses and what happens after acceptance (e.g., evidence becomes stale).

## Resolved Questions

- **Audit findings auto-trigger Librarian review**: Yes — when `EvidenceChainBroken` or `PolicyViolationDetected` references a memory, the Librarian re-evaluates that memory. Memories with `authority >= UserInstruction` require human confirmation before the Librarian acts.
- **Auditor human escalation**: Yes — for `Critical` severity violations and repeated violations (same role, same violation type, N occurrences). Non-critical findings stay internal. Critical escalations publish `HumanFeedbackRequested` via the Intent Lead.
