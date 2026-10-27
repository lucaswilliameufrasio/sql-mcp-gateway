#!/bin/sh
set -eu

vault status -address="$VAULT_ADDR" >/dev/null

if ! vault secrets list -format=json | grep -q '"database/"'; then
    vault secrets enable -path=database database >/dev/null
fi

vault policy write sql-mcp-gateway-runtime - <<'POLICY'
path "database/creds/*" {
  capabilities = ["read"]
}

path "sys/leases/revoke" {
  capabilities = ["update"]
}

POLICY

runtime_environment=/vault-runtime/runtime.env
runtime_token=
if [ -f "$runtime_environment" ]; then
    runtime_token=$(sed -n 's/^VAULT_RUNTIME_TOKEN=//p' "$runtime_environment")
fi

if [ -n "$runtime_token" ] && VAULT_TOKEN="$runtime_token" vault token lookup >/dev/null 2>&1; then
    unset runtime_token
    echo "Vault database engine, policy, and local runtime token are ready."
    exit 0
fi

runtime_token=$(vault token create \
    -orphan \
    -no-default-policy \
    -policy=sql-mcp-gateway-runtime \
    -ttl=720h \
    -display-name=sql-mcp-gateway-local-runtime \
    -field=token)
temporary_environment="${runtime_environment}.tmp.$$"
printf 'VAULT_RUNTIME_TOKEN=%s\n' "$runtime_token" > "$temporary_environment"
chmod 600 "$temporary_environment"
mv "$temporary_environment" "$runtime_environment"
unset runtime_token

echo "Vault database engine, policy, and restricted local runtime token are ready."
