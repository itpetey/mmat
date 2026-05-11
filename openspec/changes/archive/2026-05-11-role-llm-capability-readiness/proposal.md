## Why

The workbench can start roles, but users cannot tell whether those roles are running with real LLM/tool capability or deterministic fallback behaviour. A working UI needs explicit capability/status visibility and safer role execution defaults.

## What Changes

- Add visible provider/tool/fallback status for roles.
- Define richer role contracts for workbench-dispatched tasks.
- Make Worker execution safety and repository write behaviour clear in the UI.
- Surface when LLM/tool clients are missing, degraded, or unavailable.

## Capabilities

### New Capabilities
- `role-capability-readiness`: Role capability status, LLM/tool configuration visibility, fallback disclosure, and safer role contracts.

### Modified Capabilities

## Impact

- Affects role construction/configuration, workbench status panels, task contract generation, worker safety affordances, and documentation.
