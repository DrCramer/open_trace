# syntax=docker/dockerfile:1
# OpenTrace Server image (build context: repository root).
#   docker build -f server/opentrace-server/packaging/docker/Dockerfile -t opentrace-server .
FROM rust:1-slim-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends build-essential && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY core ./core
COPY server ./server
RUN cargo build --release --locked --manifest-path server/opentrace-server/Cargo.toml \
    && cp server/opentrace-server/target/release/opentrace-server /opentrace-server

FROM debian:bookworm-slim
ENV OPENTRACE_RESOURCES=/app/resources \
    OPENTRACE_SERVER_DATA_DIR=/data \
    OPENTRACE_SERVER_LISTEN=0.0.0.0:8770 \
    OPENTRACE_SERVER_TRANSPORT=http
WORKDIR /app
COPY --from=build /opentrace-server /usr/local/bin/opentrace-server
COPY resources ./resources
RUN mkdir -p /data && chown -R 10001:10001 /data
USER 10001:10001
EXPOSE 8770
VOLUME ["/data"]
# Inside the container the port is published to the host's loopback by compose; TLS is the proxy's job.
ENV OPENTRACE_SERVER_ALLOW_INSECURE_HTTP=true
CMD ["opentrace-server", "serve"]
