FROM rust:1.85.1 as builder

WORKDIR /app

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY .sqlx/ ./.sqlx/
COPY models/ ./models/
COPY dto/ ./dto/
COPY commands/ ./commands/
COPY queries/ ./queries/
COPY common/ ./common/
COPY platform/ ./platform/
COPY realtime/ ./realtime/
COPY worker/ ./worker/
COPY gateway/ ./gateway/
COPY agent-engine/ ./agent-engine/

# Build backend API
RUN cargo build --release --all

FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/backend-api /app/backend
COPY --from=builder /app/target/release/console-api /app/console
COPY --from=builder /app/target/release/realtime /app/realtime
COPY --from=builder /app/target/release/gateway /app/gateway


EXPOSE 3001
