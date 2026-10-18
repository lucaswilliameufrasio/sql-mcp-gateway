#!/bin/sh
set -eu

tls_directory=/var/run/postgresql/dev-tls
mkdir -p "$tls_directory"
cp /certs/ca.crt "$tls_directory/ca.crt"
cp /certs/server.crt "$tls_directory/server.crt"
cp /certs/server.key "$tls_directory/server.key"
chown postgres:postgres "$tls_directory" "$tls_directory/ca.crt" "$tls_directory/server.crt" "$tls_directory/server.key"
chmod 700 "$tls_directory"
chmod 644 "$tls_directory/ca.crt" "$tls_directory/server.crt"
chmod 600 "$tls_directory/server.key"

exec /usr/local/bin/docker-entrypoint.sh "$@"
