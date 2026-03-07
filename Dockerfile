# syntax=docker/dockerfile:1

FROM rust:1.93-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY models/Cargo.toml models/Cargo.toml
COPY dto/Cargo.toml dto/Cargo.toml
COPY commands/Cargo.toml commands/Cargo.toml
COPY queries/Cargo.toml queries/Cargo.toml
COPY common/Cargo.toml common/Cargo.toml
COPY platform/Cargo.toml platform/Cargo.toml
COPY worker/Cargo.toml worker/Cargo.toml
COPY agent-engine/Cargo.toml agent-engine/Cargo.toml
COPY oauth-relay/Cargo.toml oauth-relay/Cargo.toml
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
WORKDIR /app
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --locked --recipe-path recipe.json

COPY Cargo.toml Cargo.lock ./
COPY .sqlx/ ./.sqlx/
COPY models/ ./models/
COPY dto/ ./dto/
COPY commands/ ./commands/
COPY queries/ ./queries/
COPY common/ ./common/
COPY platform/ ./platform/
COPY worker/ ./worker/
COPY agent-engine/ ./agent-engine/
COPY oauth-relay/ ./oauth-relay/

# Build only the binaries this image serves.
RUN cargo build --release --locked \
    -p platform --bin backend-api --bin console-api --bin oauth-api --bin gateway-api --bin realtime-api \
    -p worker --bin worker

FROM debian:bookworm-slim
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    curl \
    gnupg \
    wget \
    && rm -rf /var/lib/apt/lists/*
RUN (curl -Ls --tlsv1.2 --proto "=https" --retry 3 https://cli.doppler.com/install.sh || wget -t 3 -qO- https://cli.doppler.com/install.sh) | sh

COPY --from=builder /app/target/release/backend-api /app/backend
COPY --from=builder /app/target/release/console-api /app/console
COPY --from=builder /app/target/release/oauth-api /app/oauth-api
COPY --from=builder /app/target/release/realtime-api /app/realtime
COPY --from=builder /app/target/release/gateway-api /app/gateway
COPY --from=builder /app/target/release/worker /app/worker

EXPOSE 3001
ENTRYPOINT ["doppler", "run", "--"]
