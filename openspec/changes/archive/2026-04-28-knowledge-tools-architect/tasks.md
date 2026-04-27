## 1. Architect Prompt Enhancement (Simplified Approach)

- [x] 1.1 Update architect system prompt to indicate knowledge search is available
- [x] 1.2 Add knowledge groups info to architect prompt

## 2. Tool-Enabled Architect Step

- [x] 2.1 Create step_with_knowledge_tools in architect.rs with tool registry support
- [x] 2.2 Wire in workflow - pass knowledge backend to architect step
- [x] 2.3 Pass materialised groups to ArchitectInput

## 3. Knowledge Lint Validator

- [x] 3.1 Add knowledge_lint check function to knowledge.rs
- [x] 3.2 Add lint findings to KnowledgeFinding enum
- [x] 3.3 Added lint check stub (validates plan structure, duplicates, empty sources)
- [x] 3.4 Update KnowledgeFinding enum for lint findings

## 4. Testing

- [x] 4.1 Add unit test for knowledge search prompt hint
- [x] 4.2 Add integration test for architect with knowledge groups on prompt

## 5. Verification

- [x] 5.1 Run cargo fmt --all
- [x] 5.2 Run cargo clippy -- -D warnings
- [x] 5.3 Run cargo test