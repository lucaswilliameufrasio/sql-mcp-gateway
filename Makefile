.PHONY: setup migrate-create migrate-up migrate-down migrate-status fmt lint test test-integration build run run-stdio admin-dev dev-up dev-app dev-status dev-pause dev-resume

setup:
	@command -v cargo >/dev/null 2>&1 || { echo "Rust/Cargo is required; install the stable Rust toolchain first." >&2; exit 1; }
	@command -v rustc >/dev/null 2>&1 || { echo "rustc is required; install the stable Rust toolchain first." >&2; exit 1; }
	@command -v node >/dev/null 2>&1 || { echo "Node.js is required; install it before running make setup." >&2; exit 1; }
	@command -v pnpm >/dev/null 2>&1 || { echo "pnpm is required; install it before running make setup." >&2; exit 1; }
	cargo fetch --locked
	@if ! command -v sqlx >/dev/null 2>&1; then \
		echo "Installing SQLx CLI..."; \
		cargo install sqlx-cli --no-default-features --features postgres,rustls; \
	fi
	@command -v sqlx >/dev/null 2>&1 || { echo "sqlx is not on PATH; add Cargo's bin directory and rerun make setup." >&2; exit 1; }
	cd apps/admin && pnpm install --frozen-lockfile
	@if [ -e .env ]; then \
		echo ".env already exists; leaving it unchanged."; \
	else \
		umask 077; cp .env.example .env; \
		echo "Created .env from .env.example with owner-only permissions."; \
	fi
	@echo "Local project dependencies are ready. Configure .env with your local PostgreSQL, OIDC, and Vault settings."

migrate-create:
	@if [ -z "$(name)" ]; then echo "Usage: make migrate-create name=add_table"; exit 1; fi
	sqlx migrate add -r "$(name)" --source crates/gateway/migrations

# SQLx CLI reads DATABASE_URL from the root .env file when it is not exported.
migrate-up:
	sqlx migrate run --source crates/gateway/migrations

migrate-down:
	sqlx migrate revert --source crates/gateway/migrations

migrate-status:
	sqlx migrate info --source crates/gateway/migrations

fmt:
	cargo fmt --all
	cd apps/admin && pnpm exec prettier --write .

lint:
	cargo clippy --workspace --all-targets -- -D warnings
	cd apps/admin && pnpm check && pnpm lint

test:
	sh scripts/test-ecs-mi-sandbox-bootstrap.sh
	cargo test --workspace
	cd apps/admin && pnpm test

test-integration:
	sh scripts/test-integration.sh

build:
	cargo build --workspace --release
	cd apps/admin && pnpm build

run:
	cargo run -p sql-mcp-gateway

run-stdio:
	cargo run -p sql-mcp-gateway -- --stdio

admin-dev:
	cd apps/admin && pnpm dev

dev-up:
	sh scripts/dev-stack.sh up

dev-app:
	sh scripts/dev-stack.sh app

dev-status:
	sh scripts/dev-stack.sh status

dev-pause:
	sh scripts/dev-stack.sh pause

dev-resume:
	sh scripts/dev-stack.sh resume
