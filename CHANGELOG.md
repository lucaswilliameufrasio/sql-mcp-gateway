# Changelog

All notable changes to SQL MCP Gateway will be documented in this file.

## [0.3.0] - 2026-11-05

### Bug Fixes

- *(release)* Preserve TOML newline during version bump

- *(release)* Normalize generated changelog whitespace


### Chores

- *(deps)* Refresh compatible Rust and admin dependencies


### Features

- Expand SQL gateway workflows and database connectivity

- *(admin)* Redesign operator workflows and add usage metrics

## [0.2.0] - 2026-10-25

### Bug Fixes

- *(mcp)* Normalize transport headers


### CI / Build

- Add pull request validation workflow

- *(release)* Add binary release workflows


### Documentation

- *(deployment)* Clarify setup and simplify OpenTofu inputs


### Features

- Implement self-hosted SQL MCP gateway

- *(deploy)* Add VPS and AWS deployment templates

- *(dev)* Add local setup target

- *(dev)* Add Authentik-backed local stack

- *(dev)* Provision local OIDC and admin via Compose

- *(ecs-managed-instances)* Add validation sandbox

- *(deploy)* Add production Docker Compose deployment

- *(auth)* Add OAuth onboarding for MCP clients

- *(mcp)* Add Streamable HTTP and remote stdio


### Styling

- *(opentofu)* Format deployment examples
