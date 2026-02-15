FROM rust:1.93-bookworm as builder

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
COPY oauth-relay/ ./oauth-relay/

# Build backend API
RUN cargo build --release --all

FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    gnupg \
    wget \
    && rm -rf /var/lib/apt/lists/*
RUN (curl -Ls --tlsv1.2 --proto "=https" --retry 3 https://cli.doppler.com/install.sh || wget -t 3 -qO- https://cli.doppler.com/install.sh) | sh

COPY --from=builder /app/target/release/backend-api /app/backend
COPY --from=builder /app/target/release/console-api /app/console
COPY --from=builder /app/target/release/realtime /app/realtime
COPY --from=builder /app/target/release/gateway /app/gateway
COPY --from=builder /app/target/release/worker /app/worker

EXPOSE 3001

# Keep command selection external (backend/console/realtime/gateway/worker),
# but always run through Doppler for secret injection.
ENTRYPOINT ["doppler", "run", "--"]
