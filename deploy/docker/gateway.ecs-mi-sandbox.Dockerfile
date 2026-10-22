FROM rust:1.94-bookworm AS builder

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --locked --release -p sql-mcp-gateway

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates libgcc-s1 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --no-create-home --user-group --shell /usr/sbin/nologin sqlgateway

COPY .dev/ecs-mi-sandbox/tls-root-ca.crt /usr/local/share/ca-certificates/sqlgw-sandbox-ca.crt
COPY .dev/ecs-mi-sandbox/rds-global-bundle.pem /tmp/aws-rds-global-bundle.pem
RUN set -eu; \
    awk '\
      /-----BEGIN CERTIFICATE-----/ {\
        certificate++;\
        output = sprintf("/usr/local/share/ca-certificates/aws-rds-global-%02d.crt", certificate);\
      }\
      output != "" { print > output }\
      /-----END CERTIFICATE-----/ { close(output); output = "" }\
      END { if (certificate == 0) exit 1 }\
    ' /tmp/aws-rds-global-bundle.pem; \
    update-ca-certificates; \
    rm -f /tmp/aws-rds-global-bundle.pem

COPY --from=builder /src/target/release/sql-mcp-gateway /usr/local/bin/sql-mcp-gateway

USER 10001:10001
ENV BIND_ADDRESS=0.0.0.0:8080
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/sql-mcp-gateway"]
