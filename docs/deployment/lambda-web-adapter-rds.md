# AWS Lambda Web Adapter with RDS PostgreSQL

The OpenTofu example packages the Rust HTTP gateway as a Lambda container image
with AWS Lambda Web Adapter (LWA), attaches it to existing private subnets, and
publishes it through a **private API Gateway REST API**. The control-plane RDS
PostgreSQL instance (or RDS Proxy), VPC/subnets/security groups, `execute-api`
interface endpoint, Vault, target databases, and admin UI are existing
resources and are not created by the example.

The MCP endpoint is request/response JSON today: the gateway accepts POST
requests with `application/json` and returns JSON; it does not implement
Streamable HTTP SSE/GET sessions. LWA/API Gateway compatibility is plausible
for this bounded request/response mode, but this repository has **not** tested
the combination. Validate protocol headers, request-size limits, timeouts,
OIDC flow, and cold-start behavior in an isolated account before production.

## Network and identity prerequisites

- Lambda private subnets must have routes/security-group egress to the RDS
  control database (or RDS Proxy), Vault, every registered PostgreSQL/MySQL
  target, and the OIDC issuer/JWKS. The RDS security group must accept TCP/5432
  from the Lambda security group.
- The Lambda bootstrap retrieves secrets from Secrets Manager during runtime
  initialization. Provide a Secrets Manager interface endpoint in the VPC or
  NAT/approved egress for that AWS API. A VPC-attached Lambda has no internet
  access merely because a selected subnet is public. If the OIDC provider is
  public, its HTTPS discovery/JWKS endpoints also need a working egress path.
- Create an `execute-api` interface VPC endpoint. Its security group must allow
  TCP/443 from approved VPN clients and/or the internal reverse proxy. Route
  private DNS from those clients to the VPC endpoint. The API resource policy
  in the example accepts invocation only through that exact endpoint.
- API Gateway authorization is `NONE` because the gateway validates OIDC
  bearer tokens itself. Do not remove the source-VPCE policy or expose the
  endpoint publicly.
- This example does not deploy the admin UI. The UI uses same-origin relative
  `/v1/*` URLs, so serve it behind an approved internal reverse proxy that
  routes `/v1/*` (and, for MCP clients, `/mcp`) to the private API URL, or
  implement and validate an equivalent same-origin arrangement. Register the
  browser origin plus `/auth/callback` with the OIDC client. Do not enable
  permissive cross-origin access as a substitute for routing.

## Secret contract

The sample image includes [`deploy/lambda/bootstrap.py`](../../deploy/lambda/bootstrap.py).
Before starting the gateway binary, it calls Secrets Manager using the Lambda
execution role and passes values to the child process through its environment.
OpenTofu stores only secret ARNs in Lambda configuration and IAM policy; do not
read secret values through OpenTofu data sources or place them in `.tfvars`.

Create these secrets outside this example:

1. `control_database_url_secret_arn`: `SecretString` is the PostgreSQL URL for
   the dedicated control-plane database (or its RDS Proxy endpoint), with
   hostname verification/TLS enabled. Use a gateway-specific role, not the RDS
   master user. The current application applies SQLx migrations on startup, so
   that role needs the required migration privileges, limited to this dedicated
   database/schema. Create the login using your secure database-administration
   workflow, set its password interactively (for example, with `psql`'s
   `\password` command), and store the URL in Secrets Manager through an
   approved secret-input channel. Never put the password in a SQL file, shell
   argument, OpenTofu variable, or repository.
2. `vault_credentials_secret_arn`: `SecretString` is a JSON object with
   `admin_token` and `runtime_token` string fields. The tokens must be distinct;
   keep the runtime token restricted to issuing and revoking approved leases.

The Lambda role can read only those secret ARNs; add `secret_kms_key_arns` only
for customer-managed keys, and ensure each KMS key policy also permits the
Lambda execution role to decrypt. Bootstrap failures log the variable
name/error class, not the resolved values. Rotated secrets are read by new
execution environments; plan a safe environment/version refresh for secret
rotation.

## Build and publish the image

The provided image selects x86-64. Update both Lambda architecture and build
platform together before using another architecture:

```sh
docker buildx build --platform linux/amd64 \
  -f deploy/docker/lambda.Dockerfile \
  -t "$ECR_REPOSITORY_URI:$RELEASE_TAG" --push .
```

Pin `lambda_image_uri` to the immutable ECR digest. Review the pinned AWS
Lambda Web Adapter and Boto3 versions in the Dockerfile/requirements file as
part of dependency updates.

## Configure and review OpenTofu

Start in [`examples/opentofu/lambda-web-adapter-rds/`](../../examples/opentofu/lambda-web-adapter-rds/):

```sh
cp terraform.tfvars.example terraform.tfvars
```

Replace the private subnet/security group IDs, VPC endpoint ID, secret ARNs,
image digest, identity settings, and Vault address. Use encrypted, restricted,
locked OpenTofu state; review and commit the generated `.terraform.lock.hcl`
for reproducible provider selection. The example creates the Lambda function,
its execution role/policies, a private REST API, a stage with access logs, and
API Gateway's invoke permission. It does not configure the regional API
Gateway account CloudWatch role; configure that once in the owning platform
stack before expecting execution logs.

Run the non-deploying checks and inspect the plan:

```sh
tofu fmt -check -recursive
tofu init
tofu validate
tofu plan
```

The example's API Gateway integration timeout is 29 seconds, the Lambda
timeout defaults to 28 seconds, and `MAX_QUERY_DURATION_SECONDS` defaults to
18 seconds. Keep enough margin for authorization, database setup, and response
serialization. Increasing only the query limit can make API Gateway return a
504 while Lambda continues work.

The application currently configures each control PostgreSQL pool for up to 20
connections. With the example's reserved concurrency of 5, budget up to roughly
100 client connections, plus other services and migration/startup activity.
This is a safety ceiling, not a measured demand estimate. Set reserved
concurrency from the RDS connection budget; an RDS Proxy does not eliminate the
Lambda-to-proxy client connection count. Monitor RDS connection metrics and
validate pool/proxy behavior with the actual SQLx workload.

## Validation gates before production

After your controlled apply, verify from a VPN-approved network that the
private invoke URL is reachable only through the configured VPC endpoint;
check `/health` and `/readyz`, the browser OIDC callback, authenticated admin
API calls, and representative bounded MCP queries. Verify that requests from
outside the endpoint are denied, secret rotation is handled, cold starts and
scale-out stay within timeout/connection budgets, and the control DB restore
procedure works. The current startup migration behavior and fixed pool limit
make these mandatory gates, not assumptions.

Complete the shared [production-readiness checklist](../production-readiness.md).

## References

- [AWS Lambda Web Adapter container images](https://aws.github.io/aws-lambda-web-adapter/getting-started/docker-images.html)
- [AWS Lambda Web Adapter environment variables](https://aws.github.io/aws-lambda-web-adapter/configuration/environment-variables.html)
- [Lambda VPC connectivity](https://docs.aws.amazon.com/lambda/latest/dg/configuration-vpc.html)
- [Private API Gateway REST APIs](https://docs.aws.amazon.com/apigateway/latest/developerguide/apigateway-private-apis.html)
- [RDS PostgreSQL](https://docs.aws.amazon.com/AmazonRDS/latest/UserGuide/CHAP_PostgreSQL.html)
