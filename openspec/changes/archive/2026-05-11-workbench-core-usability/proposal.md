## Why

The current workbench proves that live runtime events can drive a browser UI, but it lacks the core controls needed to operate a project over time. Users need reliable project/run state, event inspection, reset controls, and non-fragile status surfaces.

## What Changes

- Add first-class workbench project/run/session state instead of a fixed `SELIUM` placeholder.
- Add project creation, reset/archive, and run selection controls.
- Add event history inspection, filtering, and raw JSON views.
- Make role, task, notification, and DAG status robust across running/failed/completed states.

## Capabilities

### New Capabilities
- `workbench-core`: Core workbench project, run, event history, and operational UI behaviours.

### Modified Capabilities

## Impact

- Affects workbench state projection, API routes, frontend controls, runtime context metadata, and documentation.
