# syntax=docker/dockerfile:1
# OpenTrace — тестовый образ для Docker Compose.
# Этап 1 собирает необязательное Rust-ядро (wheel, abi3 для Python >= 3.10);
# без него (--build-arg WITH_RUST_CORE=0) приложение работает на чистом Python,
# результат анализа тот же, только медленнее.
FROM python:3.12-slim-bookworm AS core
ARG WITH_RUST_CORE=1
WORKDIR /src
COPY core ./core
RUN mkdir -p /wheels && if [ "$WITH_RUST_CORE" = "1" ]; then \
      apt-get update \
      && apt-get install -y --no-install-recommends curl ca-certificates build-essential \
      && curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable \
      && . /root/.cargo/env \
      && pip install --no-cache-dir "maturin>=1.5,<2" \
      && maturin build --release --locked -m core/opentrace-py/Cargo.toml -o /wheels; \
    fi

FROM python:3.12-slim-bookworm
ENV PYTHONUNBUFFERED=1 \
    PYTHONDONTWRITEBYTECODE=1 \
    HOME=/tmp \
    OPENTRACE_HOST=0.0.0.0
WORKDIR /app
COPY --from=core /wheels /wheels
COPY pyproject.toml ./
COPY genemapper_open ./genemapper_open
RUN pip install --no-cache-dir . \
    && if ls /wheels/*.whl >/dev/null 2>&1; then pip install --no-cache-dir /wheels/*.whl; fi \
    && rm -rf /wheels \
    && python -c "import genemapper_open._core as c; print('Rust core:', c.available())" \
    && mkdir -p /data/projects /data/security && chown -R 10001:10001 /data
USER 10001:10001
EXPOSE 8765
VOLUME ["/data"]
HEALTHCHECK --interval=15s --timeout=5s --start-period=10s --retries=5 \
  CMD python -c "import urllib.request,sys; sys.exit(0 if urllib.request.urlopen('http://127.0.0.1:8765/api/health',timeout=4).status==200 else 1)"
CMD ["python", "-m", "genemapper_open", "serve", "--port", "8765", \
     "--projects-dir", "/data/projects", "--security-dir", "/data/security"]
