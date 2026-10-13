FROM rust:1.90-bookworm AS builder

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --locked --release -p sql-mcp-gateway

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates libgcc-s1 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --no-create-home --user-group --shell /usr/sbin/nologin sqlgateway

COPY --from=builder /src/target/release/sql-mcp-gateway /usr/local/bin/sql-mcp-gateway

USER 10001:10001
ENV BIND_ADDRESS=0.0.0.0:8080
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/sql-mcp-gateway"]
