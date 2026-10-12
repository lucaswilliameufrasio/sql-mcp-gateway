#!/bin/sh
set -eu
umask 077

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
temporary_root=/tmp/opencode
temporary_directory=$(mktemp -d "$temporary_root/sql-mcp-gateway-it.XXXXXX")
tls_directory="$temporary_directory/tls"
compose_environment="$temporary_directory/compose.env"
test_environment="$temporary_directory/test.env"
project_name="sql-mcp-gateway-it-$(basename "$temporary_directory" | tr -cd 'a-z0-9')"
stack_started=0

mkdir -m 755 "$tls_directory"

compose() {
    docker compose \
        --project-name "$project_name" \
        --file "$repo_root/compose.integration.yaml" \
        --env-file "$compose_environment" \
        "$@"
}

cleanup() {
    exit_status=$?
    if [ "$stack_started" -eq 1 ]; then
        compose down --volumes --remove-orphans >/dev/null 2>&1 || true
    fi
    rm -rf -- "$temporary_directory"
    exit "$exit_status"
}

trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

command -v docker >/dev/null 2>&1 || {
    echo "Docker CLI is required for the integration suite" >&2
    exit 1
}
docker compose version >/dev/null
command -v openssl >/dev/null 2>&1 || {
    echo "OpenSSL is required to create temporary integration TLS certificates" >&2
    exit 1
}

openssl rand -hex 32 > "$temporary_directory/vault-token"
openssl rand -hex 32 > "$temporary_directory/postgres-password"
openssl rand -hex 32 > "$temporary_directory/mysql-password"
vault_token=$(cat "$temporary_directory/vault-token")
postgres_password=$(cat "$temporary_directory/postgres-password")
mysql_password=$(cat "$temporary_directory/mysql-password")
rm "$temporary_directory/vault-token" "$temporary_directory/postgres-password" "$temporary_directory/mysql-password"

openssl req -x509 -newkey rsa:2048 -nodes \
    -keyout "$tls_directory/ca.key" \
    -out "$tls_directory/ca.crt" \
    -days 2 \
    -subj "/CN=SQL MCP Gateway Integration CA" \
    -addext "basicConstraints=critical,CA:TRUE" \
    -addext "keyUsage=critical,keyCertSign,cRLSign" \
    >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes \
    -keyout "$tls_directory/server.key" \
    -out "$tls_directory/server.csr" \
    -subj "/CN=postgres" \
    >/dev/null 2>&1
cat > "$tls_directory/server.ext" <<'EOF'
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
subjectAltName=DNS:postgres,DNS:mysql,IP:127.0.0.1
EOF
openssl x509 -req \
    -in "$tls_directory/server.csr" \
    -CA "$tls_directory/ca.crt" \
    -CAkey "$tls_directory/ca.key" \
    -set_serial 1 \
    -out "$tls_directory/server.crt" \
    -days 2 \
    -extfile "$tls_directory/server.ext" \
    >/dev/null 2>&1
chmod 600 "$tls_directory/ca.key"
chmod 644 "$tls_directory/server.key" "$tls_directory/server.crt" "$tls_directory/ca.crt"

cat > "$compose_environment" <<EOF
VAULT_DEV_ROOT_TOKEN_ID=$vault_token
POSTGRES_PASSWORD=$postgres_password
MYSQL_ROOT_PASSWORD=$mysql_password
INTEGRATION_TLS_DIR=$tls_directory
EOF
chmod 600 "$compose_environment"

stack_started=1
compose up --detach --wait

vault_port=$(compose port vault 8200)
postgres_port=$(compose port postgres 5432)
mysql_port=$(compose port mysql 3306)
vault_port=${vault_port##*:}
postgres_port=${postgres_port##*:}
mysql_port=${mysql_port##*:}

cat > "$test_environment" <<EOF
SQL_MCP_GATEWAY_TEST_VAULT_ADDRESS=http://127.0.0.1:$vault_port/
SQL_MCP_GATEWAY_TEST_VAULT_ADMIN_TOKEN=$vault_token
SQL_MCP_GATEWAY_TEST_POSTGRES_PASSWORD=$postgres_password
SQL_MCP_GATEWAY_TEST_POSTGRES_PORT=$postgres_port
SQL_MCP_GATEWAY_TEST_MYSQL_PASSWORD=$mysql_password
SQL_MCP_GATEWAY_TEST_MYSQL_PORT=$mysql_port
SQL_MCP_GATEWAY_TEST_TLS_CA=$tls_directory/ca.crt
EOF
chmod 600 "$test_environment"

iteration=1
while [ "$iteration" -le 2 ]; do
    echo "Running live Vault/PostgreSQL/MySQL integration pass $iteration of 2"
    SQL_MCP_GATEWAY_TEST_ENV="$test_environment" \
        CARGO_INCREMENTAL=0 \
        cargo test \
            --manifest-path "$repo_root/Cargo.toml" \
            -p sql-mcp-gateway \
            --bin sql-mcp-gateway \
            handlers::tests::live_vault_postgres_mysql_dynamic_leases_enforce_database_permissions \
            -- --ignored --exact --nocapture
    iteration=$((iteration + 1))
done

compose down --volumes --remove-orphans
stack_started=0
echo "Live integration suite passed twice; local services and generated credentials were removed."
