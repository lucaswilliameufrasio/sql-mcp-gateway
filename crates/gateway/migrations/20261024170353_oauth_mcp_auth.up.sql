CREATE TABLE mcp_oauth_clients (
    client_id TEXT PRIMARY KEY,
    client_name TEXT NOT NULL,
    redirect_uris JSONB NOT NULL CHECK (jsonb_typeof(redirect_uris) = 'array'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE mcp_oauth_requests (
    id UUID PRIMARY KEY,
    upstream_state_hash BYTEA NOT NULL UNIQUE,
    client_id TEXT NOT NULL REFERENCES mcp_oauth_clients(client_id) ON DELETE CASCADE,
    redirect_uri TEXT NOT NULL,
    client_state TEXT NOT NULL,
    client_code_challenge TEXT NOT NULL,
    resource TEXT NOT NULL,
    principal_id UUID REFERENCES principals(id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX mcp_oauth_requests_expiry_idx ON mcp_oauth_requests (expires_at);

CREATE TABLE mcp_oauth_codes (
    code_hash BYTEA PRIMARY KEY,
    client_id TEXT NOT NULL REFERENCES mcp_oauth_clients(client_id) ON DELETE CASCADE,
    principal_id UUID NOT NULL REFERENCES principals(id) ON DELETE CASCADE,
    redirect_uri TEXT NOT NULL,
    code_challenge TEXT NOT NULL,
    resource TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX mcp_oauth_codes_expiry_idx ON mcp_oauth_codes (expires_at);

CREATE TABLE mcp_oauth_tokens (
    token_hash BYTEA PRIMARY KEY,
    token_type TEXT NOT NULL CHECK (token_type IN ('access', 'refresh')),
    token_family_id UUID NOT NULL,
    client_id TEXT NOT NULL REFERENCES mcp_oauth_clients(client_id) ON DELETE CASCADE,
    principal_id UUID NOT NULL REFERENCES principals(id) ON DELETE CASCADE,
    resource TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX mcp_oauth_tokens_principal_idx ON mcp_oauth_tokens (principal_id, expires_at);
CREATE INDEX mcp_oauth_tokens_family_idx ON mcp_oauth_tokens (token_family_id);
