-- Add down migration script here
DROP TABLE audit_events;
DROP TABLE principal_database_grants;
DROP TABLE database_targets;
DROP TABLE principals;

DROP TYPE platform_role;
DROP TYPE access_level;
DROP TYPE database_engine;
