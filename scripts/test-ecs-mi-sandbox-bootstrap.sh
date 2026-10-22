#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
bootstrap_template="$repo_root/examples/opentofu/ecs-managed-instances-sandbox/support-host-bootstrap.sh.tftpl"
install_line=$(grep -E '^dnf install -y ' "$bootstrap_template" || true)

if [ -z "$install_line" ]; then
    echo "Support-host bootstrap package installation command was not found." >&2
    exit 1
fi

case " $install_line " in
    *" curl-minimal "*) ;;
    *)
        echo "Support-host bootstrap must use the preinstalled curl-minimal package." >&2
        exit 1
        ;;
esac

case " $install_line " in
    *" curl "*)
        echo "Support-host bootstrap must not request the conflicting full curl package." >&2
        exit 1
        ;;
    *) ;;
esac

if ! grep -Fq 'chmod 0644 "$compose_dir/deploy/authentik/sql-mcp-gateway-dev.yaml"' "$bootstrap_template"; then
    echo "The Authentik blueprint must be readable by the worker through its read-only mount." >&2
    exit 1
fi

if grep -Fq 'chmod 600 "$root_dir/config.json" "$compose_dir/scripts/dev-vault-bootstrap.sh" "$compose_dir/deploy/authentik/sql-mcp-gateway-dev.yaml"' "$bootstrap_template"; then
    echo "The Authentik blueprint must not be owner-readable only." >&2
    exit 1
fi

if ! grep -Fq 'if [ ! -f "$env_file" ]; then' "$bootstrap_template" ||
    ! grep -Fq 'vault_root_token=$(sed -n '\''s/^VAULT_DEV_ROOT_TOKEN_ID=//p'\'' "$env_file")' "$bootstrap_template"; then
    echo "Bootstrap retries must preserve existing generated environment credentials." >&2
    exit 1
fi

if ! grep -Fq 'if [ "$tls_material_complete" -eq 1 ]; then' "$bootstrap_template" ||
    ! grep -Fq 'TLS material is incomplete; refusing to replace an existing test CA.' "$bootstrap_template"; then
    echo "Bootstrap retries must preserve complete TLS material and fail closed on partial material." >&2
    exit 1
fi

if ! grep -Fq 'restart authentik-worker' "$bootstrap_template"; then
    echo "Bootstrap retries must restart an existing Authentik worker after fixing blueprint access." >&2
    exit 1
fi

echo "Support-host bootstrap package, blueprint access, and retry-safety checks passed."
