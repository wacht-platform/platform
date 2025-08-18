FROM rust:1.85.1 as build

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY .sqlx/ ./.sqlx/
COPY models/ ./models/
COPY dto/ ./dto/
COPY commands/ ./commands/
COPY queries/ ./queries/
COPY common/ ./common/
COPY platform/ ./platform/
COPY worker/ ./worker/
COPY realtime/ ./realtime/
COPY gateway/ ./gateway/

RUN cargo build --release --bin realtime
RUN cargo build --release --bin worker
RUN cargo build --release --bin gateway

RUN cargo build --release --bin platform --features console-api
RUN cp target/release/platform target/release/console

RUN cargo build --release --bin platform --features backend-api --no-default-features
RUN cp target/release/platform target/release/backend

RUN cargo build --release --bin platform --features frontend-api --no-default-features
RUN cp target/release/platform target/release/frontend

FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY entrypoint.sh /app/entrypoint.sh
COPY --from=build /app/target/release/console /app/console
COPY --from=build /app/target/release/backend /app/backend
COPY --from=build /app/target/release/frontend /app/frontend
COPY --from=build /app/target/release/realtime /app/realtime
COPY --from=build /app/target/release/worker /app/worker
COPY --from=build /app/target/release/gateway /app/gateway

RUN chmod +x /app/entrypoint.sh /app/console /app/backend /app/frontend /app/realtime /app/worker /app/gateway

EXPOSE 8973

ENTRYPOINT ["/app/entrypoint.sh"]
CMD ["console"]
