# Deployment guide index

Use this page to choose a deployment shape, then follow its guide from prerequisites through verification. Deployment is not just the gateway binary: the Rust gateway and SvelteKit admin UI are separate processes. Put them behind an approved private TLS origin when possible; the browser calls `/v1/*` on its current origin and MCP clients call `POST /mcp`.

| Target | Guide | What the example deploys | What you provide |
| --- | --- | --- | --- |
| VPS, new or existing Caddy/Nginx | [VPS](vps.md) | Gateway and admin UI as systemd services; proxy examples | Linux host, system users, private network, control PostgreSQL, Vault, OIDC, target DBs, TLS and secret delivery |
| Amazon ECS Managed Instances | [ECS Managed Instances](ecs-managed-instances.md) | Gateway task/service, IAM roles, capacity provider and log group | ECR image/repository, VPC, ALB target group, subnet/SG IDs, Secrets Manager entries, Vault, databases, OIDC, admin UI and listener routing |
| AWS Lambda Web Adapter + RDS PostgreSQL | [Lambda + RDS](lambda-web-adapter-rds.md) | Lambda image/function, IAM, private REST API, stage and access logs | ECR image/repository, VPC and endpoints, RDS/control DB, Vault, target DBs, Secrets Manager entries, OIDC, admin UI and same-origin routing |

## Deploy the admin UI

The UI is a separate SvelteKit Node-adapter process. The AWS examples below deploy only the gateway, so provide this process through an existing private Node runtime or your platform's service. From the repository root, build the production UI:

```sh
cd apps/admin
pnpm install --frozen-lockfile
pnpm build
```

Deploy the generated `build/` directory with the app's Node runtime dependencies and start it with `NODE_ENV=production`, `HOST`, `PORT`, and `ORIGIN` set. For a same-host reverse proxy, bind `HOST=127.0.0.1` and use `PORT=3000`; set `ORIGIN` to the exact external HTTPS origin (for example, `https://sql-gateway.example.internal`). On another private service, bind only to an approved private interface and restrict ingress to the proxy/load balancer. The origin plus `/auth/callback` must be registered in the OIDC client.

For a same-host manual smoke run from `apps/admin/` after the build, the adapter starts with:

```sh
NODE_ENV=production HOST=127.0.0.1 PORT=3000 ORIGIN=https://sql-gateway.example.internal node build
```

Route browser `/v1/*` requests to the gateway and other UI routes to this Node service. Also route `/mcp` to the gateway for MCP clients. Keep the UI and gateway behind the same trusted TLS origin where possible; do not expose the Node port or gateway port directly to the public internet.

## Run the deployment in this order

1. **Prepare shared services.** Provision a dedicated control-plane PostgreSQL database, Vault with database secrets engines for your target engines, the OIDC client/provider, target databases, private routes, DNS/TLS and an approved secret-delivery path. Do not expose database ports to MCP clients or the public internet.
2. **Choose the runtime.** Read the corresponding target guide. The AWS examples deploy only the gateway; they do not create the admin UI or shared network/database/Vault resources.
3. **Build and publish.** For AWS, build the correct architecture and push an immutable container image to an existing private ECR repository. For a VPS, build the binary and admin UI for the host's OS/architecture (or obtain matching release artifacts).
4. **Configure without putting secret values in code.** The ECS example accepts Secrets Manager ARNs. The Lambda bootstrap resolves Secrets Manager references at startup. The VPS service reads a root-managed environment file delivered by your secret manager. OpenTofu state and plans must be access-controlled and encrypted.
5. **Review before deployment.** Copy the example's `locals.auto.tf.example` to the ignored `locals.auto.tf`, edit its local values, then run the target guide's formatting, initialization, validation and saved-plan commands. Review the full plan, IAM scope, routes, connection limits, costs and state backend. The examples do not configure remote state for you; select and configure your organization's supported backend before using them for a real environment. Treat the saved plan as a private deployment artifact.
6. **Deploy only through your change process.** After approval, the operator applies/enables the reviewed release. No example is automatically applied, and none has been deployed to AWS or production-validated by this repository.
7. **Verify and onboard.** Check `/health` and `/readyz` through the intended private route; test the OIDC admin callback and authenticated API; register a target, sign in each user once, grant minimum access, and test a representative MCP request. `/readyz` checks the control-plane database only, not Vault or target databases.
8. **Complete release gates.** Work through the shared [production readiness checklist](../production-readiness.md), including backups/restore, secret rotation, TLS, monitoring, and rollback.

The HTTP MCP implementation accepts JSON-RPC over authenticated `POST /mcp`; it does not currently implement Streamable HTTP SSE or GET sessions. Confirm that the chosen MCP client and every proxy/API integration support this request/response transport before production. The Lambda + API Gateway combination is especially unverified; see its guide for mandatory isolated-account checks.

## Example files

- [`deploy/vps/`](../../deploy/vps/) — systemd units and Caddy/Nginx site examples.
- [`deploy/docker/`](../../deploy/docker/) — gateway and Lambda container build files.
- [`examples/opentofu/ecs-managed-instances/`](../../examples/opentofu/ecs-managed-instances/) — ECS Managed Instances example.
- [`examples/opentofu/lambda-web-adapter-rds/`](../../examples/opentofu/lambda-web-adapter-rds/) — Lambda Web Adapter + private REST API example.

OpenTofu reads all `*.tf` files and ignores the tracked `locals.auto.tf.example` templates. Copy a template to `locals.auto.tf` to activate it; `.gitignore` keeps that operator-specific file out of Git. Never add resolved credentials to either file. Review and commit the generated `.terraform.lock.hcl` so provider selection is reproducible.
