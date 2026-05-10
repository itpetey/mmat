## Why

The workbench currently publishes generic events and projects runtime output heuristically. To become a working UI, role mentions, task assignment, review requests, scheduler state, and Librarian activity need explicit integration semantics.

## What Changes

- Convert mention routing into role-specific contracts or guided actions.
- Make direct reviewer interactions publish review-appropriate events.
- Start and expose the Librarian service when memory processing is enabled.
- Project runtime scheduler/task state into the UI rather than relying only on UI-local DAG heuristics.
- Define auto-chaining boundaries between intent, research, planning, implementation, review, and audit.

## Capabilities

### New Capabilities
- `workbench-runtime-integration`: Workbench-to-runtime event routing, scheduler projection, and role service integration.

### Modified Capabilities

## Impact

- Affects workbench message handling, role contracts, runtime service startup, scheduler projection, and Librarian visibility.
