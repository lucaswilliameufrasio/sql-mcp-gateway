#!/bin/sh
set -eu

cp /certs/server.crt "$PGDATA/server.crt"
cp /certs/server.key "$PGDATA/server.key"
chmod 600 "$PGDATA/server.key"

cat >> "$PGDATA/postgresql.conf" <<'EOF'
ssl = on
ssl_cert_file = '/var/lib/postgresql/data/server.crt'
ssl_key_file = '/var/lib/postgresql/data/server.key'
EOF
