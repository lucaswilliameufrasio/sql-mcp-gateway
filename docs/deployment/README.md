# Deployment guides

SQL MCP Gateway has two separately deployed processes: the Rust gateway (HTTP
and MCP) and the SvelteKit administration UI (Node adapter). Route the UI and
the gateway API through one trusted origin when possible: the browser calls
`/v1/*` on its current origin and the MCP endpoint is `/mcp`.

| Target | Guide | Example artifacts |
| --- | --- | --- |
| VPS, new or existing Caddy/Nginx | [VPS](vps.md) | [`deploy/vps/`](../../deploy/vps/), [`deploy/docker/gateway.Dockerfile`](../../deploy/docker/gateway.Dockerfile) |
| Amazon ECS Managed Instances | [ECS Managed Instances](ecs-managed-instances.md) | [`examples/opentofu/ecs-managed-instances/`](../../examples/opentofu/ecs-managed-instances/), [`deploy/docker/gateway.Dockerfile`](../../deploy/docker/gateway.Dockerfile) |
| AWS Lambda Web Adapter + RDS PostgreSQL | [Lambda + RDS](lambda-web-adapter-rds.md) | [`examples/opentofu/lambda-web-adapter-rds/`](../../examples/opentofu/lambda-web-adapter-rds/), [`deploy/docker/lambda.Dockerfile`](../../deploy/docker/lambda.Dockerfile) |

The OpenTofu examples expect shared network resources, secret values, and (for
ECS) a private ALB target group to be owned by your platform stack. They only
manage the resources described in their guides. Review every variable, IAM
permission, CIDR, timeout, and service quota for your account before applying.
No example has been applied to an AWS account or validated against a production
environment.

Complete the environment-specific [production readiness checklist](../production-readiness.md)
before enabling real users.
