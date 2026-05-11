# Build stage
FROM rust:1-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    libpq-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

COPY . .

RUN cargo build --release -p mmat-workbench

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libpq5 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/mmat-workbench /usr/local/bin/mmat-workbench

ENV MMAT_WORKBENCH_ADDR=0.0.0.0:8080

EXPOSE 8080

CMD ["mmat-workbench"]
