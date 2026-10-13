FROM rust:1.90-bookworm AS builder

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --locked --release -p sql-mcp-gateway

FROM public.ecr.aws/awsguru/aws-lambda-adapter:1.1.0 AS lambda-adapter

FROM python:3.12-slim-bookworm

COPY --from=lambda-adapter /lambda-adapter /opt/extensions/lambda-adapter
COPY --from=builder /src/target/release/sql-mcp-gateway /usr/local/bin/sql-mcp-gateway
COPY deploy/lambda/bootstrap.py /app/bootstrap.py
COPY deploy/lambda/requirements.txt /app/requirements.txt

RUN python -m pip install --no-cache-dir --disable-pip-version-check --requirement /app/requirements.txt \
    && useradd --system --uid 10001 --no-create-home --user-group --shell /usr/sbin/nologin sqlgateway

USER 10001:10001
ENV BIND_ADDRESS=0.0.0.0:8080 \
    PORT=8080 \
    AWS_LWA_PORT=8080 \
    AWS_LWA_READINESS_CHECK_PATH=/health \
    AWS_LWA_READINESS_CHECK_TIMEOUT_SECONDS=20 \
    AWS_LWA_ASYNC_INIT=true

CMD ["python3", "/app/bootstrap.py"]
