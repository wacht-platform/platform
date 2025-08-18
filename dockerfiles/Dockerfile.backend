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

# Build backend API
RUN cargo build --release --bin platform --features backend-api --no-default-features

FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/platform /app/backend

RUN chmod +x /app/backend

EXPOSE 3001

CMD ["/app/backend"]
