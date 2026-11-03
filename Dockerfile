# syntax=docker/dockerfile:1
# OpenTrace — тестовый образ для Docker Compose. Один статический по зависимостям бинарник на Rust
# (без Python): сервер, CLI и расчётное ядро собираются из core/opentrace-core.
FROM rust:1-slim-bookworm AS build
RUN apt-get update \
    && apt-get install -y --no-install-recommends build-essential \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY core ./core
RUN cargo build --release --locked --manifest-path core/opentrace-core/Cargo.toml --bin opentrace \
    && cp core/target/release/opentrace /opentrace

FROM debian:bookworm-slim
ENV HOME=/tmp \
    OPENTRACE_HOST=0.0.0.0 \
    OPENTRACE_RESOURCES=/app/resources
WORKDIR /app
COPY --from=build /opentrace /usr/local/bin/opentrace
COPY resources ./resources
RUN mkdir -p /data/projects /data/security && chown -R 10001:10001 /data
USER 10001:10001
EXPOSE 8765
VOLUME ["/data"]
HEALTHCHECK --interval=15s --timeout=5s --start-period=10s --retries=5 \
  CMD ["opentrace", "health", "--port", "8765"]
CMD ["opentrace", "serve", "--port", "8765", \
     "--projects-dir", "/data/projects", "--security-dir", "/data/security"]
