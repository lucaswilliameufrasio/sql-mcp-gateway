.PHONY: migrate-create migrate-up migrate-down migrate-status fmt lint test test-integration build run run-stdio admin-dev

migrate-create:
	@if [ -z "$(name)" ]; then echo "Usage: make migrate-create name=add_table"; exit 1; fi
	sqlx migrate add -r "$(name)" --source crates/gateway/migrations

migrate-up:
	@if [ -z "$${DATABASE_URL:-}" ]; then echo "DATABASE_URL must target the control-plane PostgreSQL database"; exit 1; fi
	sqlx migrate run --source crates/gateway/migrations

migrate-down:
	@if [ -z "$${DATABASE_URL:-}" ]; then echo "DATABASE_URL must target the control-plane PostgreSQL database"; exit 1; fi
	sqlx migrate revert --source crates/gateway/migrations

migrate-status:
	@if [ -z "$${DATABASE_URL:-}" ]; then echo "DATABASE_URL must target the control-plane PostgreSQL database"; exit 1; fi
	sqlx migrate info --source crates/gateway/migrations

fmt:
	cargo fmt --all
	cd apps/admin && pnpm exec prettier --write .

lint:
	cargo clippy --workspace --all-targets -- -D warnings
	cd apps/admin && pnpm check && pnpm lint

test:
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
