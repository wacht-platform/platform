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
COPY worker/ ./worker/
COPY realtime/ ./realtime/
COPY gateway/ ./gateway/

# Build all binaries
RUN cargo build --release --bin platform
RUN cargo build --release --bin worker  
RUN cargo build --release --bin realtime
RUN cargo build --release --bin gateway

# Build platform variants
RUN cargo build --release --bin platform --features console-api
RUN cp target/release/platform target/release/platform-console

RUN cargo build --release --bin platform --features backend-api --no-default-features
RUN cp target/release/platform target/release/platform-backend

RUN cargo build --release --bin platform --features frontend-api --no-default-features
RUN cp target/release/platform target/release/platform-frontend