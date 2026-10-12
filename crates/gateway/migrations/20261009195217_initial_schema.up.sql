-- Add up migration script here
CREATE TYPE database_engine AS ENUM ('postgresql', 'mysql');
CREATE TYPE access_level AS ENUM ('read', 'write');
CREATE TYPE platform_role AS ENUM ('admin', 'member');

CREATE TABLE principals (
    id UUID PRIMARY KEY,
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    email TEXT,
    display_name TEXT,
    platform_role platform_role NOT NULL DEFAULT 'member',
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    last_seen_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (issuer, subject)
);

CREATE TABLE database_targets (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    engine database_engine NOT NULL,
    host TEXT NOT NULL,
    port INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
    database_name TEXT NOT NULL,
    schema_name TEXT NOT NULL DEFAULT 'public',
    tls_mode TEXT NOT NULL CHECK (tls_mode IN ('verify_full', 'verify_ca', 'disable')),
    tls_ca_pem TEXT,
    vault_connection_name TEXT NOT NULL UNIQUE,
    vault_read_role TEXT NOT NULL UNIQUE,
    vault_write_role TEXT UNIQUE,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_by UUID NOT NULL REFERENCES principals(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE principal_database_grants (
    principal_id UUID NOT NULL REFERENCES principals(id) ON DELETE CASCADE,
    database_id UUID NOT NULL REFERENCES database_targets(id) ON DELETE CASCADE,
    access_level access_level NOT NULL,
    granted_by UUID NOT NULL REFERENCES principals(id),
    granted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (principal_id, database_id)
);

CREATE TABLE audit_events (
    id UUID PRIMARY KEY,
    principal_id UUID REFERENCES principals(id) ON DELETE SET NULL,
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    database_id UUID REFERENCES database_targets(id) ON DELETE SET NULL,
    operation TEXT NOT NULL,
    access_level access_level,
    outcome TEXT NOT NULL CHECK (outcome IN ('success', 'denied', 'failure')),
    error_code TEXT,
    query_fingerprint TEXT,
    database_principal TEXT,
    vault_lease_fingerprint TEXT,
    duration_ms BIGINT CHECK (duration_ms IS NULL OR duration_ms >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX audit_events_created_at_idx ON audit_events (created_at DESC);
CREATE INDEX audit_events_principal_created_at_idx ON audit_events (principal_id, created_at DESC);
CREATE INDEX principal_database_grants_database_id_idx ON principal_database_grants (database_id);
