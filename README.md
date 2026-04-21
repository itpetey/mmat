# MMAT

MMAT, short for **Make Me A Thing**, is an interactive planning and implementation tool for repository-based software work.

It opens a browser-based chat UI, asks what you want to build, explores several solution directions with an LLM, asks you to approve the recommended approach, then plans and executes the work in isolated git worktrees before merging validated changes back into your checkout.

Today the execution pipeline is opinionated toward Rust projects because the built-in validation steps run Cargo commands.

## What MMAT does

- Starts with a free-form prompt such as a feature request, refactor, or product idea.
- Runs a discovery stage over the current repository and, optionally, external web research.
- Generates three candidate solution branches by default: conservative, recommended, and ambitious.
- Reconciles those branches into one proposal and asks you to approve it or request revisions.
- Builds an implementation plan and runs an architect-style review over that plan.
- Executes implementation items in isolated git worktrees under `.mmat-worktrees`.
- Validates each item with `cargo fmt --all`, `cargo check`, `cargo test`, and `cargo clippy -- -D warnings`.
- Runs a final review and, when needed, schedules remediation passes before finishing.

## Requirements

- Rust toolchain
- Git repository with a valid `HEAD`
- An OpenAI-compatible API endpoint
- A sibling checkout of the NAAF repository at `../naaf/main`

MMAT uses path dependencies from `../naaf/main`, so this repository is not currently self-contained.

## Configuration

MMAT reads these environment variables:

| Variable | Purpose | Default |
| --- | --- | --- |
| `OPENAI_API_KEY` | API key for the LLM endpoint | `lm-studio` |
| `OPENAI_BASE_URL` | Base URL for an OpenAI-compatible API | `http://127.0.0.1:1234/v1` |
| `OPENAI_MODEL` | Model name used for all workflow stages | `essentialai/rnj-1` |
| `OPENAI_ORG_ID` | Optional OpenAI organisation id | unset |
| `MMAT_WEB_SEARCH_URL` | Optional web search endpoint | unset |
| `MMAT_WEB_SEARCH_API_KEY` | Optional API key for the web search endpoint | unset |
| `WEB_SEARCH_URL` | Fallback for `MMAT_WEB_SEARCH_URL` | unset |
| `WEB_SEARCH_API_KEY` | Fallback for `MMAT_WEB_SEARCH_API_KEY` | unset |

If no web search URL is configured, MMAT still runs, but external research is disabled.

## Build

Clone this repository and ensure the NAAF checkout exists at `../naaf/main`, then build normally:

```bash
cargo build
```

For a release build:

```bash
cargo build --release
```

## Usage

MMAT uses the **current working directory** as the project root it will inspect and modify.

If you run `cargo run` inside this repository, MMAT will operate on this repository. If you want to use MMAT on another project, run the built binary from inside that other repository.

### Interactive mode (default)

```bash
cargo run
```

This starts a local server and prints a URL to stdout. Open that URL in your browser to interact with MMAT through a chat interface. The server stays running until you press `Ctrl+C`.

To bypass the browser prompt and start a workflow immediately:

```bash
cargo run -- --prompt "Add a CLI flag to export the generated plan as JSON."
```

### Non-interactive mode

Use `--prompt` to start a workflow without the browser UI, or `--resume` to continue a previous run:

```bash
cargo run -- --prompt "Your prompt here"
cargo run -- --resume .mmat/runs/run-123
```

To print all run artefact paths after completion:

```bash
cargo run -- --prompt "Your prompt" --export-artifacts
```

From there MMAT will:

1. Inspect the repository and build a discovery brief.
2. Explore three solution branches by default.
3. Present a reconciled proposal and ask for approval or revisions.
4. Plan the implementation.
5. Execute validated changes.
6. Leave the merged result in your working tree.

## Operational Notes

- MMAT creates temporary worktrees in `.mmat-worktrees` while it is implementing tasks.
- It copies the current workspace state into those worktrees, so uncommitted local changes are part of the working context.
- The implementation pipeline assumes Cargo is available and that `cargo fmt`, `cargo check`, `cargo test`, and `cargo clippy` are meaningful for the target repository.
- The interactive interface is browser-based via a local LiveView server. Use `--prompt` to skip the browser step.

## Development Checks

Before committing changes in this repository, run:

```bash
cargo fmt --all
cargo clippy -- -D warnings
cargo test
```

## Horizon

- Track ideas and bugs over time
- Allow injecting ideas ad hoc with a constant development loop
- Ability to feed in resources over time - research articles, code snippets etc.
- Ability to automatically parallelise development using worktrees with a reconcile step to rebase back onto active branch
