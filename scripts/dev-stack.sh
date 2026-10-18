#!/bin/sh
set -eu
umask 077

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
env_file="$repo_root/.env.dev"
dev_root="$repo_root/.dev"
tls_root="$repo_root/.dev/tls"
private_tls_directory="$tls_root/private"
certificate_directory="$tls_root/certs"
runtime_token_directory="$repo_root/.dev/vault-runtime"
runtime_environment_file="$runtime_token_directory/runtime.env"
compose_file="$repo_root/compose.dev.yaml"
project_name=sql-mcp-gateway-dev
docker_context=${DOCKER_CONTEXT:-rootless}

compose() {
    docker --context "$docker_context" compose \
        --project-name "$project_name" \
        --file "$compose_file" \
        --env-file "$env_file" \
        "$@"
}

require_tools() {
    command -v docker >/dev/null 2>&1 || {
        echo "Docker CLI is required for the local Compose stack." >&2
        exit 1
    }
    docker --context "$docker_context" compose version >/dev/null 2>&1 || {
        echo "Docker context '$docker_context' is unavailable; select an approved context with DOCKER_CONTEXT." >&2
        exit 1
    }
}

prepare_environment() {
    if [ -L "$env_file" ]; then
        echo ".env.dev is a symbolic link; refusing to follow it." >&2
        exit 1
    fi
    if [ -e "$env_file" ]; then
        if [ ! -f "$env_file" ]; then
            echo ".env.dev exists but is not a regular file; refusing to replace it." >&2
            exit 1
        fi
        chmod 600 "$env_file"
        return
    fi

    command -v openssl >/dev/null 2>&1 || {
        echo "OpenSSL is required to generate local-only credentials and TLS files." >&2
        exit 1
    }

    control_db_password=$(openssl rand -hex 32)
    authentik_db_password=$(openssl rand -hex 32)
    target_postgres_password=$(openssl rand -hex 32)
    target_mysql_password=$(openssl rand -hex 32)
    authentik_secret_key=$(openssl rand -hex 48)
    authentik_bootstrap_password=$(openssl rand -hex 24)
    vault_root_token=$(openssl rand -hex 32)

    cat > "$env_file" <<EOF
CONTROL_DB_PASSWORD=$control_db_password
AUTHENTIK_DB_PASSWORD=$authentik_db_password
TARGET_POSTGRES_PASSWORD=$target_postgres_password
TARGET_MYSQL_PASSWORD=$target_mysql_password
AUTHENTIK_SECRET_KEY=$authentik_secret_key
AUTHENTIK_BOOTSTRAP_PASSWORD=$authentik_bootstrap_password
VAULT_DEV_ROOT_TOKEN_ID=$vault_root_token
OIDC_ISSUER_URL=http://127.0.0.1:9000/application/o/sql-mcp-gateway-dev/
OIDC_CLIENT_ID=sql-mcp-gateway-dev
OIDC_ACCESS_TOKEN_AUDIENCE=sql-mcp-gateway-dev
OIDC_BOOTSTRAP_ADMIN_SUBJECTS=dev-admin
EOF
    chmod 600 "$env_file"
    unset control_db_password authentik_db_password target_postgres_password target_mysql_password
    unset authentik_secret_key authentik_bootstrap_password vault_root_token
    echo "Created owner-only .env.dev with fresh local-only credentials; existing files are never overwritten."
}

prepare_runtime_environment() {
    if [ -L "$dev_root" ] || [ -L "$runtime_token_directory" ] || [ -L "$runtime_environment_file" ]; then
        echo "The local Vault runtime path contains a symbolic link; refusing to follow it." >&2
        exit 1
    fi
    if [ -e "$dev_root" ] && [ ! -d "$dev_root" ]; then
        echo "$dev_root exists but is not a directory; refusing to replace it." >&2
        exit 1
    fi
    if [ -e "$runtime_token_directory" ] && [ ! -d "$runtime_token_directory" ]; then
        echo "$runtime_token_directory exists but is not a directory; refusing to replace it." >&2
        exit 1
    fi
    mkdir -p "$runtime_token_directory"
    chmod 700 "$dev_root" "$runtime_token_directory"

    if [ -e "$runtime_environment_file" ]; then
        if [ ! -f "$runtime_environment_file" ]; then
            echo "$runtime_environment_file exists but is not a regular file; refusing to replace it." >&2
            exit 1
        fi
        chmod 600 "$runtime_environment_file"
        return
    fi

    printf 'VAULT_RUNTIME_TOKEN=\n' > "$runtime_environment_file"
    chmod 600 "$runtime_environment_file"
}

prepare_tls() {
    if [ -L "$dev_root" ] || [ -L "$tls_root" ] \
        || [ -L "$private_tls_directory" ] || [ -L "$certificate_directory" ]; then
        echo "The local TLS path contains a symbolic link; refusing to follow it." >&2
        exit 1
    fi
    if [ -e "$dev_root" ] && [ ! -d "$dev_root" ]; then
        echo "$dev_root exists but is not a directory; refusing to replace it." >&2
        exit 1
    fi

    if [ -d "$tls_root" ]; then
        if [ -f "$tls_root/.generated-by-dev-stack" ] \
            && [ -f "$private_tls_directory/ca.key" ] \
            && [ -f "$certificate_directory/ca.crt" ] \
            && [ -f "$certificate_directory/server.crt" ] \
            && [ -f "$certificate_directory/server.key" ]; then
            if [ -L "$tls_root/.generated-by-dev-stack" ] \
                || [ -L "$private_tls_directory/ca.key" ] \
                || [ -L "$certificate_directory/ca.crt" ] \
                || [ -L "$certificate_directory/server.crt" ] \
                || [ -L "$certificate_directory/server.key" ]; then
                echo "$tls_root contains a symbolic link; refusing to use it." >&2
                exit 1
            fi
            chmod 700 "$tls_root" "$private_tls_directory" "$certificate_directory"
            chmod 600 "$private_tls_directory/ca.key" "$certificate_directory/server.key"
            chmod 644 "$certificate_directory/ca.crt" "$certificate_directory/server.crt"
            return
        fi
        echo "$tls_root already exists but is not a complete stack-generated TLS directory; refusing to overwrite it." >&2
        exit 1
    fi

    mkdir -p "$private_tls_directory" "$certificate_directory"
    chmod 700 "$tls_root" "$private_tls_directory" "$certificate_directory"

    openssl req -x509 -newkey rsa:2048 -nodes \
        -keyout "$private_tls_directory/ca.key" \
        -out "$certificate_directory/ca.crt" \
        -days 365 \
        -subj "/CN=SQL MCP Gateway Local Development CA" \
        -addext "basicConstraints=critical,CA:TRUE" \
        -addext "keyUsage=critical,keyCertSign,cRLSign" \
        >/dev/null 2>&1
    openssl req -newkey rsa:2048 -nodes \
        -keyout "$certificate_directory/server.key" \
        -out "$private_tls_directory/server.csr" \
        -subj "/CN=sql-mcp-gateway-local-targets" \
        >/dev/null 2>&1
    cat > "$private_tls_directory/server.ext" <<'EOF'
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
subjectAltName=DNS:target-postgres,DNS:target-mysql
EOF
    openssl x509 -req \
        -in "$private_tls_directory/server.csr" \
        -CA "$certificate_directory/ca.crt" \
        -CAkey "$private_tls_directory/ca.key" \
        -set_serial 1 \
        -out "$certificate_directory/server.crt" \
        -days 365 \
        -extfile "$private_tls_directory/server.ext" \
        >/dev/null 2>&1
    chmod 600 "$private_tls_directory/ca.key" "$certificate_directory/server.key"
    chmod 644 "$certificate_directory/ca.crt" "$certificate_directory/server.crt"
    touch "$tls_root/.generated-by-dev-stack"
    rm "$private_tls_directory/server.csr" "$private_tls_directory/server.ext"
    echo "Created local-only database TLS material under .dev/tls."
}

require_environment() {
    if [ ! -f "$env_file" ]; then
        echo "Local Compose settings are missing; run 'make dev-up' first." >&2
        exit 1
    fi
}

start_infrastructure() {
    compose up --detach --wait \
        authentik-postgres \
        authentik \
        authentik-worker \
        control-postgres \
        target-postgres \
        target-mysql \
        vault
    compose exec --user 0 --no-TTY vault /bin/sh /scripts/dev-vault-bootstrap.sh
}

require_tools

case "${1:-}" in
    up)
        prepare_environment
        prepare_tls
        prepare_runtime_environment
        compose config --quiet
        compose --profile app config --quiet
        start_infrastructure
        echo "Local dependencies are running. Configure the Authentik OIDC application before 'make dev-app'."
        ;;
    app)
        require_environment
        prepare_tls
        prepare_runtime_environment
        compose --profile app config --quiet
        start_infrastructure
        compose --profile app up --detach --build --wait gateway
        echo "Gateway is running on http://127.0.0.1:8080. Start the admin UI with 'make admin-dev'."
        ;;
    status)
        require_environment
        compose ps --all
        ;;
    pause)
        require_environment
        compose pause
        ;;
    resume)
        require_environment
        compose unpause
        ;;
    *)
        echo "Usage: sh scripts/dev-stack.sh {up|app|status|pause|resume}" >&2
        exit 2
        ;;
esac
