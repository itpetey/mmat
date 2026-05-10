## ADDED Requirements

### Requirement: Workbench serves separated static assets
The workbench SHALL serve HTML, CSS, and JavaScript from separate asset files instead of one inline Rust string.

#### Scenario: Browser requests index
- **WHEN** a browser requests `/`
- **THEN** the workbench MUST return the static `index.html`
- **AND** the HTML MUST reference separate CSS and JavaScript assets

#### Scenario: Browser requests CSS and JS
- **WHEN** a browser requests the referenced CSS or JavaScript asset
- **THEN** the workbench MUST return the correct content with an appropriate content type

### Requirement: Backend source excludes frontend implementation details
The Rust workbench server SHALL contain routing, API, SSE, projection, and runtime wiring, not large inline frontend markup or scripts.

#### Scenario: Server source is reviewed
- **WHEN** a developer opens `crates/workbench/src/main.rs`
- **THEN** it MUST NOT contain the full HTML/CSS/JS application as a large string constant

### Requirement: Static assets are available in release runs
The workbench SHALL be able to serve static assets in normal development and release execution modes.

#### Scenario: Release binary starts
- **WHEN** the release workbench binary starts from the documented working directory
- **THEN** `/`, CSS, and JavaScript asset routes MUST respond successfully
