## ADDED Requirements

### Requirement: UI renders a multi-column shell for domain-mapped projects
The system SHALL render a 3-column layout for projects that have a domain tree, replacing the single-column conversation view. The layout MUST include a left sidebar (domain tree + delivery graph), a centre content area (tabbed sub-domain conversations), and a right details panel (per-node contextual information). Projects without a domain tree MUST continue to use the existing single-column layout.

#### Scenario: Domain-mapped project shows 3-column shell
- **WHEN** a project has a `DomainTree` with one or more nodes
- **THEN** the UI MUST render the multi-column shell with sidebar, tabbed centre, and detail panel
- **AND** the single-column `.mmat-conversation` layout MUST NOT be rendered

#### Scenario: Simple project shows single-column shell
- **WHEN** a project does not have a `DomainTree`
- **THEN** the UI MUST render the existing single-column `.mmat-conversation` layout unchanged

### Requirement: Domain tree is a navigable sidebar component
The system SHALL render the domain tree in the left sidebar as a nested, indented tree with status badges per node. Clicking a node MUST open or focus its corresponding conversation tab in the centre panel.

#### Scenario: Domain tree shows all nodes with status
- **WHEN** a domain tree exists with multiple nodes
- **THEN** each node MUST display its name, status badge (○ waiting, ● active, ✓ complete, ⚠ backflow), and depth via indentation
- **AND** parent-child relationships MUST be visually evident through nesting

#### Scenario: Clicking a node opens its tab
- **WHEN** the user clicks a domain node in the tree
- **THEN** the node's sub-domain conversation tab MUST open in the centre panel
- **AND** if the tab is already open, it MUST be focused (scrolled into view, selected)

#### Scenario: Empty tree shows a placeholder
- **WHEN** a domain tree exists but has no nodes yet (root discovery in progress)
- **THEN** the tree area MUST show a placeholder message such as "Mapping domains..."

### Requirement: Sub-domain conversations use in-app tabs
The system SHALL use in-app tabs (not browser tabs) for parallel sub-domain discovery sessions. Each sub-domain with an active or pending conversation MUST have its own tab in the centre panel. The active tab renders the conversation history, pending questions, and composer for that sub-domain.

#### Scenario: Tabs are created for active sub-domains
- **WHEN** multiple sub-domain discovery or planning sessions are active
- **THEN** each session MUST have its own tab in the centre panel's tab bar
- **AND** the tab MUST display the sub-domain name and a status indicator

#### Scenario: Switching tabs preserves conversation state
- **WHEN** the user switches between sub-domain tabs
- **THEN** the conversation history, pending questions, and composer state for the deselected tab MUST be preserved
- **AND** the newly selected tab MUST show its own conversation history and state

#### Scenario: Completed sub-domain tabs are closable
- **WHEN** a sub-domain completes all pipeline stages (discovery through delivery)
- **THEN** its tab MUST show a close button
- **AND** clicking close MUST remove the tab from the tab bar

#### Scenario: Backflow reopens or highlights affected tabs
- **WHEN** a backflow event targets a sub-domain whose tab was closed
- **THEN** the tab MUST reopen automatically with a backflow visual indicator
- **AND** the conversation history from prior stages MUST be preserved

#### Scenario: Tab bar respects open order
- **WHEN** multiple tabs are open
- **THEN** tabs MUST be ordered by the sequence in which the user opened them (most recently focused at the right)

### Requirement: Per-sub-domain conversation state is independent
The system SHALL maintain separate conversation state per sub-domain node. Each sub-domain's state MUST include its own conversation history, composer mode, pending prompts, run summary, and pipeline phase.

#### Scenario: Two sub-domains have independent conversations
- **WHEN** sub-domain A and sub-domain B both have conversation entries
- **THEN** viewing tab A MUST show only A's conversation entries
- **AND** viewing tab B MUST show only B's conversation entries

#### Scenario: User message is scoped to its sub-domain tab
- **WHEN** the user submits a reply while viewing sub-domain A's tab
- **THEN** the user message entry MUST be appended only to sub-domain A's conversation history
- **AND** sub-domain B's conversation history MUST be unchanged

### Requirement: Pipeline phase indicator shows per-sub-domain progress
The system SHALL display a breadcrumb-style phase indicator above each sub-domain's conversation, showing the sub-domain's current pipeline stage (Discovery → Knowledge → Solutions → Architect → Delivery). The current phase MUST be highlighted. Backflow MUST show the retraced path.

#### Scenario: Phase indicator shows current stage
- **WHEN** sub-domain A is in the knowledge planning phase
- **THEN** the phase indicator MUST highlight the Knowledge stage
- **AND** completed stages (Discovery) MUST appear visually distinct from pending stages

#### Scenario: Backflow shows retraced path
- **WHEN** a sub-domain backflows from Delivery to Architect
- **THEN** the phase indicator MUST show the path with a backflow marker (e.g., curved arrow or dotted line from Delivery back to Architect)
- **AND** previously completed stages that are now invalidated MUST appear in a reset state

### Requirement: Delivery graph visualisation shows batch progress
The system SHALL render a mini delivery graph in the left sidebar (below the domain tree) when a `DeliveryGraph` exists. It MUST show batches as horizontal layers with nodes colour-coded by job status.

#### Scenario: Delivery graph shows active batch
- **WHEN** a delivery graph has multiple batches and Batch 1 is executing
- **THEN** Batch 1 MUST be visually highlighted
- **AND** Batch 2 and subsequent batches MUST appear as pending

#### Scenario: Delivery graph shows node status per job
- **WHEN** a delivery batch contains multiple jobs with different statuses
- **THEN** each job node MUST be colour-coded: pending (grey), running (blue), succeeded (green), failed (red)

#### Scenario: No delivery graph shows placeholder
- **WHEN** no delivery graph exists (domains still in planning)
- **THEN** the delivery graph area MUST show "Delivery pending" or be hidden

### Requirement: Backflow notifications alert the user
The system SHALL display a backflow notification banner above the conversation area of any sub-domain affected by backflow. The banner MUST indicate severity, the affected sub-domain name, and cascade information.

#### Scenario: Backflow banner appears for affected sub-domain
- **WHEN** a sub-domain receives a backflow event
- **THEN** a banner MUST appear above the sub-domain's conversation panel
- **AND** the banner MUST indicate the severity (Moderate, Major, Critical) with distinct colour coding

#### Scenario: Cascade notification shows dependent sub-domains
- **WHEN** a Critical backflow cascades to dependent sub-domains
- **THEN** the backflow banner MUST list the dependent sub-domains that are affected
- **AND** each dependent sub-domain's tab MUST show a cascade indicator

#### Scenario: Backflow exceeding max cascade depth halts with human review notice
- **WHEN** backflow cascade depth exceeds `DomainTreeConfig::max_cascade_depth`
- **THEN** the banner MUST display a halt message indicating human review is required
- **AND** the pipeline MUST stop and await user action

### Requirement: Right detail panel shows contextual per-node information
The system SHALL render a collapsible right-side detail panel that displays contextual information about the currently selected domain node or delivery job.

#### Scenario: Detail panel shows node information
- **WHEN** a domain node is selected (via tree click or active tab)
- **THEN** the detail panel MUST show the node's status, current pipeline phase, tree depth, knowledge group count (public/private), dependency list, and backflow history

#### Scenario: Empty detail panel when nothing is selected
- **WHEN** no domain node is selected
- **THEN** the detail panel MUST show a placeholder or be collapsed

#### Scenario: Detail panel is collapsible
- **WHEN** the user clicks the collapse toggle on the detail panel
- **THEN** the panel MUST collapse to a narrow strip
- **AND** the centre content area MUST expand to fill the freed space

### Requirement: State management extends to support multi-domain data
The system SHALL extend the existing `UiState` and `UiSnapshot` types to hold domain tree data, delivery graph state, sub-domain UI states, backflow notifications, and tab management state. The existing watch-channel reactive pattern MUST be preserved.

#### Scenario: UiSnapshot carries domain tree data
- **WHEN** `UiState::snapshot()` is called for a domain-mapped project
- **THEN** the returned `UiSnapshot` MUST include the domain tree, open tabs, active domain node ID, delivery graph, and backflow notifications

#### Scenario: Sub-domain state is independent per node
- **WHEN** `UiState` manages multiple sub-domains
- **THEN** each sub-domain's state (conversation, pending prompt, composer mode, pipeline phase) MUST be stored in a `BTreeMap<DomainNodeId, DomainUiState>`
- **AND** the active domain node ID MUST not affect the state of inactive sub-domains

### Requirement: UI remains compatible with existing single-project flow
The system SHALL preserve all existing UI functionality for projects that do not use domain-mapped planning. The project switcher, queue panel, raw logs disclosure, and conversation rendering MUST work identically to the current implementation when no domain tree is present. Additionally, the composer MUST support message queuing during running steps, and the Escape key MUST provide step interruption as defined in the `step-interrupt` and `message-queue` capabilities.

#### Scenario: Existing conversation flow works unchanged
- **WHEN** a project has no domain tree
- **THEN** the user MUST be able to type an initial prompt, interact with discovery questions, select solution branches, and view streaming LLM output exactly as before

#### Scenario: Delivery queue panel remains functional
- **WHEN** a project has no domain tree
- **THEN** the `QueuePanel` component MUST render at its existing position in the single-column layout

#### Scenario: Composer accepts input during running step
- **WHEN** a step is running and the user types into the composer and submits
- **THEN** the message MUST be queued (as per message-queue capability)
- **AND** a queue indicator MUST appear showing the number of queued messages

#### Scenario: Escape key provides step interruption
- **WHEN** a step is running and the user presses Escape twice within 3 seconds
- **THEN** the step MUST be interrupted (as per step-interrupt capability)

### Requirement: First boot shows project creation form instead of default project
When the application starts and the project registry is empty, the UI SHALL display a project creation form instead of the default project conversation view. The form MUST collect a project name and root directory, and on submission create the project before showing the conversation composer.

#### Scenario: Empty registry on first boot
- **WHEN** the application starts and the project registry has no projects
- **THEN** the UI MUST render a `NewProjectForm` component instead of the composer
- **AND** the `NewProjectForm` MUST contain fields for project name and root directory
- **AND** the form MUST have a "Create Project" submit button
- **AND** the `ensure_default_project` logic MUST NOT run

#### Scenario: Project creation form submitted
- **WHEN** the user fills in the project name and root directory and clicks "Create Project"
- **THEN** the system MUST call `UiState::register_project` with the submitted values
- **AND** on success, the form MUST be replaced by the normal conversation composer
- **AND** on failure, the form MUST display the error message

#### Scenario: Registry already has projects on startup
- **WHEN** the application starts and the project registry already has one or more projects
- **THEN** the UI MUST render the normal conversation composer directly
- **AND** the project creation form MUST NOT be shown

#### Scenario: Project creation form validates input
- **WHEN** the user submits the form with an empty project name
- **THEN** the form MUST display a validation error and MUST NOT create the project

### Requirement: International English directive in all system prompts
All LLM system prompts in the planning and delivery stages SHALL include an International English directive. The directive MUST specify Oxford spelling conventions (e.g., `organise`, `realisation`, `colour`, `favour`, `metre`), avoid American spellings, and instruct the model to use International English exclusively.

#### Scenario: Discovery stage system prompt includes directive
- **WHEN** the discovery stage builds its system prompt
- **THEN** the system prompt MUST include the International English directive

#### Scenario: Solutions stage system prompt includes directive
- **WHEN** the solutions stage builds its system prompt
- **THEN** the system prompt MUST include the International English directive

#### Scenario: Knowledge stage system prompt includes directive
- **WHEN** the knowledge stage builds its system prompt
- **THEN** the system prompt MUST include the International English directive

#### Scenario: Architect stage system prompt includes directive
- **WHEN** the architect stage builds its system prompt
- **THEN** the system prompt MUST include the International English directive

#### Scenario: Delivery engine system prompts include directive
- **WHEN** the delivery engine builds any system prompt (planning, implementation, peer review, contract validation, final review)
- **THEN** each system prompt MUST include the International English directive
