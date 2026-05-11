ME:
What model/models would you use for each of the agents in this codebase?

CHATGPT:
Your architecture is unusually well suited to:
- multi-model routing,
- open/proprietary hybrid cognition,
- local/private reasoning,
- and role-specialized inference.

Most agent systems are too tightly coupled to a single provider. MMAT’s role/event/memory architecture naturally supports model pluralism.

The important shift is:

> choose models by cognitive function, not by benchmark rank.

---

# My overall recommendation

I would run MMAT as:

| Layer | Model Strategy |
|---|---|
| cheap high-volume cognition | open/self-hosted |
| institutional governance | deterministic + small local models |
| architecture/review/escalation | frontier APIs |
| implementation | mixed |
| retrieval/attention | local |
| memory governance | mostly deterministic |
| audits | mostly deterministic + selective LLM |

That gives:
- privacy,
- resilience,
- cost control,
- reduced vendor dependence,
- and independent failure modes.

---

# The most important architectural recommendation

Do NOT bind roles directly to providers.

Instead:

```rust
trait CognitiveProfile {
    fn reasoning_depth(&self) -> ReasoningDepth;
    fn latency_class(&self) -> LatencyClass;
    fn privacy_class(&self) -> PrivacyClass;
    fn context_requirement(&self) -> usize;
    fn tool_reliability_required(&self) -> bool;
    fn hallucination_tolerance(&self) -> HallucinationTolerance;
}
```

Then route dynamically.

---

# My recommended model stack

---

# 1. Frontier reasoning layer

Use sparingly.

These are your:
- architecture,
- escalation,
- synthesis,
- critical review,
- and hard debugging models.

## Best current options

| Model | Strength |
|---|---|
| Claude Opus 4.5/4.6 | best long-horizon software engineering |
| GPT-5.5 | strongest all-round engineering judgement |
| Gemini 3.x Pro | enormous context + repo synthesis |

Claude still appears strongest for real-world SWE-agent tasks and long-horizon implementation quality.  [oai_citation:0‡CodeSOTA](https://www.codesota.com/guides/code-generation-models?utm_source=chatgpt.com)

GPT-5.x remains the best balanced “general engineering cognition” model.  [oai_citation:1‡CodeSOTA](https://www.codesota.com/guides/code-generation-models?utm_source=chatgpt.com)

Gemini becomes extremely useful for:
- huge repositories,
- large context ingestion,
- architecture understanding,
- and multimodal PRD/design flows.

---

# 2. Open-weight implementation layer

This is where MMAT gets very interesting.

Your Workers are perfect candidates for open/self-hosted models.

## My current favourites

| Model | Why |
|---|---|
| Qwen3-Coder-Next | strongest efficiency/performance ratio |
| GLM-5 / GLM-5.1 | strongest open reasoning+coding combo |
| Kimi K2.5 | excellent long-horizon coding |
| DeepSeek V3.2 | absurd cost/performance |
| MiniMax M2.5 | very strong SWE-bench performance |

Qwen3-Coder-Next is especially interesting because it was explicitly trained for coding agents with environment feedback.  [oai_citation:2‡arXiv](https://arxiv.org/abs/2603.00729?utm_source=chatgpt.com)

GLM-5 and MiniMax are probably the strongest open-weight “organisation cognition” candidates overall right now.  [oai_citation:3‡AgentMarketCap](https://agentmarketcap.ai/blog/2026/04/06/open-source-models-swe-bench-gap-2026-qwen-kimi-glm-chinese-labs?utm_source=chatgpt.com)

DeepSeek remains remarkable for:
- cost,
- speed,
- and self-hosting economics.  [oai_citation:4‡AgentMarketCap](https://agentmarketcap.ai/blog/2026/04/06/open-source-models-swe-bench-gap-2026-qwen-kimi-glm-chinese-labs?utm_source=chatgpt.com)

---

# 3. Small local cognition models

This is the layer most people underutilize.

Use small local models for:
- salience scoring,
- memory extraction,
- classification,
- routing,
- confidence estimation,
- summarisation,
- contradiction detection,
- SOP selection,
- event tagging.

Examples:
- Gemma 4
- Phi-4
- Qwen small variants
- Mistral small variants

These roles should be:
- cheap,
- local,
- fast,
- always-on.

Your Attention Engine should probably never hit a frontier API.

---

# 4. Embedding layer

Do NOT use one embedding model forever.

You’ll likely need:
- code embeddings,
- semantic memory embeddings,
- process embeddings,
- architectural embeddings.

Early recommendation:

| Concern | Recommendation |
|---|---|
| cheap/general | text-embedding-3-small |
| local/private | Qwen embedding models |
| code-heavy | code-specialized embeddings |
| graph retrieval | hybrid symbolic+vector |

Eventually:
# retrieval quality matters more than raw generation quality.

---

# Role-by-role recommendations

---

# Intent Lead

Needs:
- curiosity,
- conversational subtlety,
- ambiguity detection.

Best:
- GPT-5.4 mini
- Claude Sonnet tier
- Gemini Flash

Could be proprietary safely because:
- low token volume,
- high UX value.

---

# Scholar

This role benefits enormously from:
- huge context,
- synthesis,
- citation discipline.

Best:
- Gemini Pro
- Claude Opus
- GLM-5

Scholar should probably use:
# multiple models

Then compare evidence overlap.

---

# Architect

Most important role.

Use your best models here.

Best:
- Claude Opus
- GPT-5.5

Architecture mistakes compound downstream.

This role deserves expensive cognition.

---

# Worker

This is where open models shine.

Use:
- Qwen3-Coder-Next
- GLM-5
- DeepSeek
- Kimi K2.5

Potentially:
- multiple workers with different models competing.

Your Reviewer selects winner.

That could become extremely powerful.

---

# Reviewer

I strongly recommend:
# model diversity

If Worker used Qwen:
- Reviewer should use Claude or GPT.

Independent failure modes matter enormously.

---

# Auditor

Mostly deterministic.

LLMs only for:
- semantic inconsistency,
- contradiction analysis,
- forensic review,
- confidence analysis.

Auditor should NOT become:
- “another reasoning agent.”

---

# Librarian

Mostly:
- symbolic,
- graph-based,
- deterministic.

LLM assistance only for:
- summarisation,
- clustering,
- supersession proposals.

---

# One architecture I think would work VERY well

## Multi-model adversarial cognition

Example:

```text
Worker A → Qwen
Worker B → DeepSeek
Worker C → Claude

Reviewer → GPT-5.5

Auditor → deterministic + GLM-5 forensic mode
```

Then:
- compare patches,
- compare assumptions,
- compare evidence,
- compare architectural consistency.

This is much stronger than trusting one model lineage.

---

# Self-hosting recommendations

If you self-host:

## Strong candidates

| Model | Hardware profile |
|---|---|
| Qwen3-Coder-Next | surprisingly efficient |
| DeepSeek V3.x | MoE efficiency |
| GLM-5 | heavier but elite |
| Gemma small models | cheap infra roles |

You’ll probably end up with:

```text
local cognition cluster
    +
frontier escalation APIs
```

That hybrid architecture is likely optimal.

---

# One very important warning

Avoid:
# model monoculture

If:
- Worker,
- Reviewer,
- Architect,
- Scholar

all use the same model family,
you amplify:
- identical hallucinations,
- identical blind spots,
- identical architectural taste,
- identical bugs.

Diversity improves organisational cognition.

---

# Another recommendation

Track:
# model provenance

For every:
- claim,
- memory,
- ADR,
- review,
- patch.

Example:

```json
{
  "produced_by_model": "qwen3-coder-next",
  "reviewed_by_model": "claude-opus-4.6",
  "audited_by": "deterministic-policy-engine-v2"
}
```

This becomes incredibly valuable later.

---

# My overall recommendation for MMAT specifically

I’d probably build around:

| Role | Suggested stack |
|---|---|
| Attention | local small models |
| Memory | deterministic + local |
| Worker | open-weight |
| Reviewer | frontier |
| Architect | frontier |
| Scholar | mixed |
| Auditor | deterministic-first |
| Ops | mostly deterministic + frontier escalation |

That architecture gives you:
- scalability,
- resilience,
- cost control,
- privacy,
- and stronger organisational cognition.

And importantly:
MMAT’s architecture is already becoming structurally compatible with this kind of heterogeneous cognition system.
