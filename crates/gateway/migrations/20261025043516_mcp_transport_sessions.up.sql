CREATE TABLE mcp_transport_sessions (
    id UUID PRIMARY KEY,
    principal_id UUID NOT NULL REFERENCES principals(id) ON DELETE CASCADE,
    protocol_version TEXT NOT NULL,
    initialized BOOLEAN NOT NULL DEFAULT FALSE,
    stream_until TIMESTAMPTZ,
    expires_at TIMESTAMPTZ NOT NULL DEFAULT NOW() + INTERVAL '30 minutes'
);
CREATE INDEX mcp_transport_sessions_principal_idx ON mcp_transport_sessions (principal_id);
CREATE INDEX mcp_transport_sessions_expiry_idx ON mcp_transport_sessions (expires_at);
