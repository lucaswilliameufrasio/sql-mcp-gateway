export type PlatformRole = 'admin' | 'member';
export type DatabaseEngine = 'postgresql' | 'mysql';
export type DatabaseTlsMode = 'verify_full' | 'disable';
export type AccessLevel = 'read' | 'write';

export interface Principal {
  id: string;
  issuer: string;
  subject: string;
  email?: string;
  display_name?: string;
  platform_role: PlatformRole;
  enabled: boolean;
  last_seen_at?: string;
}

export interface DatabaseGrant {
  database_id: string;
  database_name: string;
  access_level: AccessLevel;
  granted_at?: string;
}

export interface MeResponse {
  principal: Principal;
  database_grants: DatabaseGrant[];
}

export interface DatabaseTarget {
  id: string;
  name: string;
  engine: DatabaseEngine;
  host: string;
  port: number;
  database_name: string;
  schema_name: string;
  tls_mode: DatabaseTlsMode;
  enabled: boolean;
  created_at: string;
  access_level?: AccessLevel;
}

export interface AuditEvent {
  id: string;
  principal_id?: string;
  issuer: string;
  subject: string;
  database_id?: string;
  operation: string;
  access_level?: AccessLevel;
  outcome: 'success' | 'denied' | 'failure';
  error_code?: string;
  query_fingerprint?: string;
  database_principal?: string;
  vault_lease_fingerprint?: string;
  duration_ms?: number;
  created_at: string;
}

export interface AuthConfig {
  issuer: string;
  client_id: string;
  resource: string;
  scopes: string[];
  mcp_endpoint: string;
  oauth_authorization_metadata_url: string;
}
