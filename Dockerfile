# Use official Rust image as builder
FROM rust:1.89 as builder

# Set working directory
WORKDIR /app

# Copy dependency files first (for better caching)
COPY Cargo.toml Cargo.lock ./

# Create a dummy main.rs to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs

# Build dependencies (this layer will be cached)
RUN cargo build --release

# Remove dummy main.rs
RUN rm src/main.rs

# Copy actual source code
COPY src ./src

# Copy assets and config
COPY assets ./assets
COPY config.json ./

# Build the actual application
RUN cargo build --release

# Runtime stage - smaller base image
FROM debian:bookworm-slim

# Install required system dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create app directory
WORKDIR /app

# Copy the binary from builder stage
COPY --from=builder /app/target/release/assistant .

# Copy assets and config
COPY --from=builder /app/assets ./assets
COPY --from=builder /app/config.json .

# Create a non-root user for security
RUN useradd -r -s /bin/false appuser && chown -R appuser:appuser /app
USER appuser

# Expose port (DO requires this even though we don't use HTTP)
EXPOSE 8080

# Run the application
CMD ["./assistant"]