FROM rust:1.85.1 as build

WORKDIR /app

# Copy workspace configuration
COPY Cargo.toml Cargo.lock ./

# Copy SQLx metadata for compile-time verification
COPY .sqlx/ ./.sqlx/

# Copy source code for all workspace members
COPY console/ ./console/
COPY shared/ ./shared/
COPY worker/ ./worker/

# Build the console application
RUN cargo build --release --bin console

# Use a smaller runtime image
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Copy the console binary from the build stage
COPY --from=build /app/target/release/console /app/console

# Expose the port the application will run on
EXPOSE 3001

# Run the console application
CMD ["./console"]
