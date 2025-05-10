# Docker Compose integration for Nginx load balancer
# Place this snippet in your docker-compose.yml at the root of your project
#
# services:
#   nginx:
#     image: nginx:latest
#     container_name: nginx-lb
#     ports:
#       - "80:80"
#     volumes:
#       - ./network/infra/nginx.conf:/etc/nginx/nginx.conf:ro
#     depends_on:
#       - node1
#       - node2
#       - node3
#   node1:
#     image: your-backend-image
#     container_name: node1
#     expose:
#       - "8080"
#   node2:
#     image: your-backend-image
#     container_name: node2
#     expose:
#       - "8080"
#   node3:
#     image: your-backend-image
#     container_name: node3
#     expose:
#       - "8080"

# Build stage
FROM rust:1.76.0-slim-bookworm AS builder

WORKDIR /usr/src/app

RUN apt-get update && \
    apt-get upgrade -y && \
    apt-get install -y --no-install-recommends \
        pkg-config \
        libssl-dev \
        protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY . .

RUN cargo build --release

# Production runtime stage
FROM debian:bookworm-slim AS runtime

WORKDIR /app

RUN apt-get update && \
    apt-get upgrade -y && \
    apt-get install -y --no-install-recommends \
        ca-certificates \
        libssl3 \
        curl \
    && rm -rf /var/lib/apt/lists/*

# Tạo user không phải root
RUN useradd -m appuser
RUN mkdir -p /app/configs /app/security

# Copy binary và cấu hình thực tế
COPY --from=builder /usr/src/app/target/release/network /app/network
COPY configs/config.yaml /app/configs/config.yaml
COPY security/configs/ /app/security/

RUN chown -R appuser:appuser /app
USER appuser

ENV RUST_LOG=info
ENV CONFIG_PATH=/app/configs/config.yaml
ENV SECURITY_ENABLED=true
ENV RATE_LIMIT_ENABLED=true
ENV AUTH_MIDDLEWARE_ENABLED=true
ENV INPUT_VALIDATION_ENABLED=true
ENV SECURITY_CONFIG_PATH=/app/security

EXPOSE 8080 9001 9100

HEALTHCHECK --interval=30s --timeout=30s --start-period=5s --retries=3 \
  CMD curl -f http://localhost:8080/health || exit 1

CMD ["/app/network"]
