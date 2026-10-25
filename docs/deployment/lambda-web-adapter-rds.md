# Deploy with AWS Lambda Web Adapter and RDS PostgreSQL

This procedure builds the gateway as a Lambda container image, configures a private REST API, reviews the OpenTofu plan, and verifies the private route. The example creates the Lambda function/role/policies, a private API Gateway REST API, stage, access-log group, and invoke permission. It does **not** create the ECR repository, VPC/subnets/security groups, RDS/RDS Proxy, `execute-api` or Secrets Manager endpoints, Vault, target databases, OIDC provider, or admin UI. Deploy the UI separately using the [shared admin UI instructions](README.md#deploy-the-admin-ui).

> **Compatibility is not yet proven.** The gateway currently accepts JSON-RPC with authenticated `POST /mcp` and `application/json`; it does not implement Streamable HTTP SSE or GET sessions. This repository has not tested this Lambda Web Adapter + API Gateway combination. Validate protocol headers, payload limits, timeout behavior, OIDC, cold starts, and concurrency in an isolated AWS account before production.

## Before you start

Prepare these shared resources first:

- A supported AWS account/Region, an AWS deployment identity with permission to create the example resources and pass the Lambda role, and an existing private ECR repository.
- At least two private Lambda subnets in a VPC with network routes/security-group egress to RDS or RDS Proxy, Vault, every target database, OIDC discovery/JWKS, and Secrets Manager. RDS must accept TCP/5432 from the Lambda security group. A VPC-attached Lambda does not gain internet access just because a subnet is public; use the Secrets Manager interface endpoint or approved NAT/proxy routes.
- An `execute-api` interface VPC endpoint for private API clients. Its security group must allow TCP/443 from approved VPN users or the internal reverse proxy. Configure private DNS/routing from those clients to the endpoint. The API resource policy allows invocation only through the configured endpoint; do not remove that condition or publish a public API.
- A dedicated control-plane PostgreSQL database. Its gateway role needs the migration privileges required at startup, limited to that database/schema; do not use the RDS master account. Budget connections for Lambda concurrency and other clients.
- Two Secrets Manager entries: (1) a `SecretString` containing the control database URL with TLS/hostname verification enabled; (2) a JSON `SecretString` with distinct `admin_token` and `runtime_token` fields for Vault. The runtime token should only issue/revoke approved leases. The example stores only ARNs in Lambda configuration and IAM.
- Vault with the database secrets engines configured, target PostgreSQL/MySQL databases, an HTTPS Vault endpoint, OIDC issuer/client/audience, and the exact bootstrap administrator OIDC `sub`.
- A separately deployed admin UI and a trusted same-origin route for browser requests. This example does not deploy the UI. The UI must reach `/v1/*` on its own origin; route those paths and `/mcp` to the private API, and register the browser origin plus `/auth/callback` with OIDC. Do not substitute permissive CORS for the trusted routing boundary.

If a secret uses a customer-managed KMS key, supply its ARN and update the KMS key policy to allow the Lambda execution role to decrypt. The example also adds a runtime deny for the Lambda function's VPC ENI-management calls while retaining the AWS-managed VPC access role policy required by Lambda's service.

## Deploy the gateway

### 1. Build and push an immutable image

The example selects x86-64. Set non-secret ECR values in your deployment shell:

```sh
export AWS_REGION=us-east-1
export AWS_ACCOUNT_ID=123456789012
export ECR_REPOSITORY=sql-mcp-gateway-lambda
export RELEASE_TAG=replace-with-an-immutable-release-tag
```

Create the repository once if it is not already owned by your platform stack:

```sh
aws ecr create-repository --region "$AWS_REGION" --repository-name "$ECR_REPOSITORY" \
  --image-tag-mutability IMMUTABLE --encryption-configuration encryptionType=AES256
```

Authenticate and build/push from the repository root:

```sh
aws ecr get-login-password --region "$AWS_REGION" \
  | docker login --username AWS --password-stdin "$AWS_ACCOUNT_ID.dkr.ecr.$AWS_REGION.amazonaws.com"

docker buildx build --platform linux/amd64 \
  -f deploy/docker/lambda.Dockerfile \
  -t "$AWS_ACCOUNT_ID.dkr.ecr.$AWS_REGION.amazonaws.com/$ECR_REPOSITORY:$RELEASE_TAG" \
  --push .
```

Get the immutable digest and use it to form `lambda_image_uri`:

```sh
aws ecr describe-images --region "$AWS_REGION" --repository-name "$ECR_REPOSITORY" \
  --image-ids imageTag="$RELEASE_TAG" --query 'imageDetails[0].imageDigest' --output text
```

Use `<account>.dkr.ecr.<region>.amazonaws.com/<repository>@sha256:<digest>`, not the mutable tag. If changing architecture, update the Lambda `architectures` setting and build platform together. Review the pinned Lambda Web Adapter and Boto3 versions during dependency updates.

### 2. Set operator-local OpenTofu values

From the example directory:

```sh
cd examples/opentofu/lambda-web-adapter-rds
cp locals.auto.tf.example locals.auto.tf
```

Edit `locals.auto.tf`: replace the image digest, private subnet/security group IDs, `execute-api` endpoint ID, secret ARNs (including the at-least-32-byte `MCP_OAUTH_STATE_KEY`), OIDC values, exact bootstrap subject, Vault URL, and the external HTTPS `mcp_public_url`. Allow `${mcp_public_url}/oauth/callback` as an exact OIDC client redirect URI and keep the state key stable across invocations/deployments. Tune memory, timeout, reserved concurrency, query duration, API throttles, stage name, and log retention. The file is ignored by Git; never put secret contents in it. These values include internal resource identifiers and the OIDC subject, so do not publish a customized copy.

Before `tofu init`, configure your organization's encrypted, access-controlled, locked remote state backend in this example directory. The example intentionally does not choose a backend. Keep the generated `.terraform.lock.hcl` under version control for reproducible provider selection.

### 3. Format, initialize, validate, and inspect the plan

Run from `examples/opentofu/lambda-web-adapter-rds/`:

```sh
tofu fmt -check -recursive
tofu init
tofu validate
tofu plan -out=reviewed.tfplan
```

Review the exact Lambda image/architecture, private API resource policy and VPC endpoint, IAM access to only the two secret ARNs, KMS key policies, subnet/security-group egress, reserved concurrency, CloudWatch role/logging, integration timeout, and expected costs. The example sets the API Gateway integration timeout to 29 seconds, Lambda timeout to 28 seconds, query duration to 18 seconds, and reserved concurrency to 5. Keep time for authentication, database setup, and response serialization; raising only the query limit can produce a 504 while Lambda continues running.

The gateway's control-plane pool allows up to 20 connections per execution environment. With concurrency set to 5, budget up to roughly 100 Lambda-to-database/proxy connections, plus other services and migration/startup activity. This is a ceiling, not a measured demand estimate. Set concurrency from the actual RDS connection budget and validate SQLx/RDS Proxy behavior with representative requests.

### 4. Deploy only after your plan is approved

Applying is an operator action, not an automatic step in this guide. After your change-control process approves the exact plan, run `tofu apply reviewed.tfplan` from the example directory. Keep the saved plan private and delete it through your normal secure artifact-retention process after use. This creates IAM, Lambda, and API Gateway resources and can incur AWS charges. The example does not configure the regional API Gateway account CloudWatch role; configure that once in the owning platform stack before expecting API execution logs.

### 5. Verify from an approved private client network

Inspect the outputs, then call the private stage URL from a client that routes through the configured `execute-api` VPC endpoint:

```sh
tofu output
API_URL="$(tofu output -raw private_api_invoke_url)"
curl -i "$API_URL/health"
curl -i "$API_URL/readyz"
```

Expect `204` from both. `/readyz` tests the control database only. Check the `/aws/lambda/<function-name>` CloudWatch log group for startup/secret-bootstrap failures and the API access log group for private API requests. From a network outside the allowed VPC endpoint, confirm invocation is denied.

Next, verify the admin UI's same-origin `/v1/*` route and OIDC callback, sign in with the bootstrap administrator, register a target database, have each user sign in once, and grant minimum per-user/per-database access. Test representative authenticated MCP `POST /mcp` requests through the intended proxy/API route. Do not send resolved secrets or bearer tokens in diagnostic logs or command arguments.

## Upgrade and rollback

Publish each release with a new immutable tag and pin `lambda_image_uri` to its digest. Review/apply the image change through the same plan approval process and retain the previous digest. To roll back, restore that digest in `locals.auto.tf`, review the plan, and apply it through change control. Exercise secret rotation, cold-start/scale-out behavior, timeout boundaries, database restore, and migration compatibility before production; the Lambda/API integration remains untested by this repository.

Complete the shared [production readiness checklist](../production-readiness.md).

## References

- [AWS Lambda Web Adapter container images](https://aws.github.io/aws-lambda-web-adapter/getting-started/docker-images.html)
- [AWS Lambda Web Adapter environment variables](https://aws.github.io/aws-lambda-web-adapter/configuration/environment-variables.html)
- [Lambda VPC connectivity](https://docs.aws.amazon.com/lambda/latest/dg/configuration-vpc.html)
- [Private API Gateway REST APIs](https://docs.aws.amazon.com/apigateway/latest/developerguide/apigateway-private-apis.html)
- [RDS PostgreSQL](https://docs.aws.amazon.com/AmazonRDS/latest/UserGuide/CHAP_PostgreSQL.html)
