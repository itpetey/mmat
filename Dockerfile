# syntax=docker/dockerfile:1.7

FROM rust:1-trixie AS rust-base

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace/mmat

FROM rust-base AS dev

RUN cargo install cargo-watch --locked

ENV RUST_LOG=info \
    MMAT_DATA_PATH=/data/mmat \
    MMAT_QDRANT_URL=http://qdrant:6333 \
    MMAT_EMBEDDING_BASE_URL=https://api.openai.com/v1 \
    MMAT_EMBEDDING_MODEL=text-embedding-3-small \
    MMAT_EMBEDDING_DIMENSION=1536 \
    MMAT_LLM_BASE_URL=http://host.docker.internal:1234/v1 \
    MMAT_DELIVERY_BIN=/workspace/mmat/target/debug/delivery

CMD ["cargo", "watch", "-w", "src", "-w", "web", "-w", "Cargo.toml", "-w", "Cargo.lock", "-x", "build --bin delivery", "-x", "run --bin frontend -- --addr 0.0.0.0:8080"]

FROM rust-base AS builder

WORKDIR /workspace/mmat
COPY Cargo.toml Cargo.lock rustfmt.toml ./
COPY src ./src
COPY web ./web

RUN cargo build --release --bin frontend --bin delivery

FROM debian:trixie-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 mmat \
    && mkdir -p /data/mmat \
    && chown -R mmat:mmat /data/mmat

COPY --from=builder /workspace/mmat/target/release/frontend /usr/local/bin/frontend
COPY --from=builder /workspace/mmat/target/release/delivery /usr/local/bin/delivery

USER mmat
WORKDIR /data/mmat

ENV RUST_LOG=info \
    MMAT_DATA_PATH=/data/mmat \
    MMAT_QDRANT_URL=http://qdrant:6333 \
    MMAT_EMBEDDING_BASE_URL=https://api.openai.com/v1 \
    MMAT_EMBEDDING_MODEL=text-embedding-3-small \
    MMAT_EMBEDDING_DIMENSION=1536 \
    MMAT_LLM_BASE_URL=http://host.docker.internal:1234/v1 \
    MMAT_DELIVERY_BIN=/usr/local/bin/delivery

EXPOSE 8080

CMD ["frontend", "--addr", "0.0.0.0:8080"]
