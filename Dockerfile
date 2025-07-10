FROM rust:1.85.1 as build

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY .sqlx/ ./.sqlx/
COPY console/ ./console/
COPY shared/ ./shared/
COPY worker/ ./worker/
COPY realtime/ ./realtime/
COPY rate-limiter/ ./rate-limiter/

RUN cargo build --release --bin realtime
RUN cargo build --release --bin worker
RUN cargo build --release --bin rate-limiter

RUN cargo build --release --bin console --features console-api
RUN cp target/release/console target/release/console-temp

RUN cargo build --release --bin console --features backend-api --no-default-features
RUN cp target/release/console target/release/backend

RUN cargo build --release --bin console --features frontend-api --no-default-features
RUN cp target/release/console target/release/frontend

RUN cp target/release/console-temp target/release/console

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
COPY --from=build /app/target/release/rate-limiter /app/rate-limiter

RUN chmod +x /app/entrypoint.sh /app/console /app/backend /app/frontend /app/realtime /app/worker /app/rate-limiter

EXPOSE 8973

ENTRYPOINT ["/app/entrypoint.sh"]
CMD ["console"]
