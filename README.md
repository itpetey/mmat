# MMAT

MMAT, short for **Make Me A Thing**, is a repository-oriented workflow engine for turning an open-ended software prompt into a structured delivery path.

This repository is a rewrite of the previous implementation in `../main/`. The rewrite keeps the broad workflow shape, but changes the architecture substantially:

- workflow code is grouped by subject under `src/workflow/`, not by generic artefact type,
- discovery is explicitly live and recursive,
- knowledge is planned and materialised as first-class workflow state,
- downstream stages receive only the knowledge groups they actually need.

## Workflow Shape

The rewritten workflow is organised as these stages:

1. Discovery
2. Knowledge Planning
3. Knowledge Materialisation
4. Solution Branch Fan-out
5. Solution Collect + Recommend + User Choice
6. Software Architect
7. Implementation Planning
8. Execution

The current codebase implements the workflow foundation through the architect handoff boundary.

## Current Implementation

The workflow foundation is implemented as subject-owned modules:

- `src/workflow/discovery/`
  Live recursive discovery, prompt construction, and per-turn NAAF step building.
- `src/workflow/knowledge/`
  Knowledge planning, SQLite-backed metadata persistence, deterministic materialisation, and stage-scoped knowledge sessions.
- `src/workflow/solutions/`
  Concurrent conservative, recommended, and ambitious branch generation, collection/recommendation, and live user choice.
- `src/workflow/architect/`
  Downstream Software Architect handoff and planning-ready output.
- `src/runtime/`
  Runtime interfaces for live human questions and stage prompt scoping without coupling workflow modules to a particular UI transport.

The browser UI and the full execution pipeline are still being rebuilt on top of these contracts.

## Scoped Knowledge

MMAT treats knowledge as explicit workflow state instead of one global prompt attachment.

- Discovery produces a structured handoff.
- A separate knowledge-planning stage proposes zero or more useful knowledge groups.
- A deterministic materialisation stage persists knowledge-group metadata via SQLite using `naaf-persistence-sqlite::SqliteKnowledgeGroupStore`.
- Each downstream stage receives only the materialised groups selected for that stage.

This keeps prompts narrower and makes evidence flow visible across the workflow.

## Upstream NAAF Follow-Ups

This rewrite intentionally records platform-level gaps as upstream NAAF work instead of embedding permanent MMAT-specific workarounds.

- Add first-class web and paper acquisition helpers to `naaf-knowledge`.
- Add duplicate and near-duplicate detection to `naaf-knowledge` linting and ingestion flows.

## Requirements

- Rust toolchain
- Git repository with a valid `HEAD`
- A sibling checkout of the NAAF repository at `../naaf/main`

MMAT uses path dependencies from `../naaf/main`, so this repository is not currently self-contained.

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

The current rewrite is primarily exercised through the Rust modules and unit tests in this repository.

Run the fast development service stack with Docker Compose:

```bash
cp .env.example .env
docker compose --profile dev up --build mmat-dev
```

Then open `http://127.0.0.1:8080`.

The development service bind-mounts this checkout and the sibling NAAF crates into the container, stores Cargo registry/git data and build artefacts in named volumes, and runs `cargo watch`. Source changes under `src/`, `web/`, `Cargo.toml`, or `Cargo.lock` recompile and restart the web server without rebuilding the image.

Use the same dev container for checks:

```bash
docker compose --profile dev run --rm mmat-dev cargo test
docker compose --profile dev run --rm mmat-dev cargo clippy -- -D warnings
docker compose --profile dev run --rm mmat-dev cargo fmt --all
```

Rebuild the dev image only when Dockerfile dependencies change, such as the Rust image or installed tools:

```bash
docker compose --profile dev build mmat-dev
```

For a packaged image that copies code into the container, run:

```bash
docker compose up --build mmat
```

That path is useful for production-style validation, but it requires an image rebuild after source changes.

The Compose stack includes:

- `mmat`, the LiveView web server.
- `mmat-dev`, the bind-mounted development web server.
- `qdrant`, the vector store used for materialised knowledge.
- named volumes for SQLite knowledge metadata and Qdrant data.

The Docker build uses the sibling NAAF checkout as a named build context, so keep the expected repository layout:

```text
projects/
  mmat/rewrite/
  naaf/main/
```

By default, the container connects to an OpenAI-compatible workflow LLM at `http://host.docker.internal:1234/v1`. Set `MMAT_LLM_BASE_URL` and `MMAT_LLM_API_KEY` in `.env` if your model endpoint is somewhere else.

Knowledge materialisation uses persistent storage by default:

- SQLite metadata is stored at `.mmat/knowledge.sqlite3`.
- Qdrant is reached at `http://127.0.0.1:6333`.
- Embeddings use the OpenAI-compatible embeddings API at `https://api.openai.com/v1`.

Override those defaults with:

```bash
MMAT_KNOWLEDGE_SQLITE_PATH=.mmat/knowledge.sqlite3
MMAT_QDRANT_URL=http://127.0.0.1:6333
MMAT_QDRANT_API_KEY=
MMAT_EMBEDDING_API_KEY=$OPENAI_API_KEY
MMAT_EMBEDDING_BASE_URL=https://api.openai.com/v1
MMAT_EMBEDDING_MODEL=text-embedding-3-small
MMAT_EMBEDDING_DIMENSION=1536
MMAT_KNOWLEDGE_REPO=mmat
cargo run --bin mmat
```

For Docker Compose, set the same values in `.env`; inside the Compose network Qdrant is reached through `http://qdrant:6333` and SQLite is stored at `/data/mmat/knowledge.sqlite3`.

To verify the implemented workflow foundation:

```bash
cargo test
```

## Development Checks

Before committing changes in this repository, run:

```bash
cargo fmt --all
cargo clippy -- -D warnings
cargo test
```
