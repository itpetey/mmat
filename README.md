# MMAT

MMAT, short for **Make Me A Thing**, is a repository-oriented plan engine for turning an open-ended software prompt into a structured delivery path.

## Workflow Shape

The product now has two independent flows:

- `plan`: discovery, knowledge planning/materialisation, solution branching, user selection, and software architect handoff.
- `deliver`: queued implementation planning and execution for approved handoffs.

The frontend submits approved plan handoffs to a separate delivery process over `ipc-channel`.

## Current Implementation

The plan foundation is implemented as subject-owned modules:

- `src/plan/discovery/`
  Live recursive discovery, prompt construction, and per-turn NAAF step building.
- `src/plan/knowledge/`
  Knowledge planning, SQLite-backed metadata persistence, deterministic materialisation, and stage-scoped knowledge sessions.
- `src/plan/solutions/`
  Concurrent conservative, recommended, and ambitious branch generation, collection/recommendation, and live user choice.
- `src/plan/architect/`
  Downstream Software Architect handoff and planning-ready output.
- `src/deliver/`
  SQLite-backed delivery queue, IPC messages, delivery models, worktree execution, cargo checks, and validation/review loops.

The browser UI runs from `src/bin/frontend.rs`; the delivery worker runs from `src/bin/delivery.rs`.

## Scoped Knowledge

MMAT treats knowledge as explicit plan state instead of one global prompt attachment.

- Discovery produces a structured handoff.
- A separate knowledge-planning stage proposes zero or more useful knowledge groups.
- A deterministic materialisation stage persists knowledge-group metadata via SQLite using `naaf-persistence-sqlite::SqliteKnowledgeGroupStore`.
- Each downstream stage receives only the materialised groups selected for that stage.

This keeps prompts narrower and makes evidence flow visible across the plan.

## Upstream NAAF Follow-Ups

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

Run the fast development service stack with Docker Compose:

```bash
cp .env.example .env
# Edit .env and set MMAT_HOST_PROJECT_ROOT=/path/to/target/repository
docker compose --profile dev up --build builder frontend
```

Then open `http://127.0.0.1:8080`.

The development services bind-mount this checkout and the sibling NAAF crates into the container, stores Cargo registry/git data and build artefacts in named volumes, and runs `cargo watch`. Source changes under `src/`, `web/`, `Cargo.toml`, or `Cargo.lock` recompile and restart the web server without rebuilding the image.

Set `MMAT_HOST_PROJECT_ROOT` to the host repository that MMAT should plan and deliver against. Compose mounts it at `MMAT_PROJECT_ROOT` inside the containers, defaulting to `/workspace/project`, and the frontend registers that container path as the default project. Ask the LLM about files by relative path within the repository, or by the container path under `/workspace/project`; host paths such as `/home/user/project` are not visible inside Docker. Delivery edits are written through that bind mount.

Use the same dev container for checks:

```bash
docker compose --profile dev run --rm frontend cargo test
docker compose --profile dev run --rm frontend cargo clippy -- -D warnings
docker compose --profile dev run --rm frontend cargo fmt --all
```

Rebuild the dev image only when Dockerfile dependencies change, such as the Rust image or installed tools:

```bash
docker compose --profile dev build builder frontend
```

For a packaged image that copies code into the container, run:

```bash
# Edit .env and set MMAT_HOST_PROJECT_ROOT=/path/to/target/repository
docker compose --profile prod up --build builder-prod frontend-prod
```

That path is useful for production-style validation, but it requires an image rebuild after source changes.

The development profile includes:

- `frontend`, the bind-mounted development LiveView web server.
- `builder`, a bind-mounted development builder that keeps the delivery binary available for frontend-launched IPC.
- `qdrant`, the vector store used for materialised knowledge.
- named volumes for SQLite knowledge metadata and Qdrant data.

The production profile includes:

- `frontend-prod`, the packaged LiveView web server.
- `builder-prod`, a packaged delivery-binary companion container used as a health gate for frontend-launched IPC.
- `qdrant`, the vector store used for materialised knowledge.
- named volumes for SQLite knowledge metadata and Qdrant data.

The Docker build uses the sibling NAAF checkout as a named build context, so keep the expected repository layout:

```text
projects/
  mmat/main/
  naaf/main/
```

By default, the container connects to an OpenAI-compatible plan LLM at `http://host.docker.internal:1234/v1`. Set `MMAT_LLM_BASE_URL` and `MMAT_LLM_API_KEY` in `.env` if your model endpoint is somewhere else.

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
cargo run --bin frontend
```

For Docker Compose, set the same values in `.env`; inside the Compose network Qdrant is reached through `http://qdrant:6333` and SQLite is stored at `/data/mmat/knowledge.sqlite3`.

To verify the implemented plan foundation:

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
