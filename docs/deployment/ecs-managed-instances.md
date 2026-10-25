# Deploy to Amazon ECS Managed Instances

This procedure builds and publishes the gateway image, configures the OpenTofu example, deploys its ECS service, and checks the result. The example deploys the **gateway only**. It creates an ECS cluster, Managed Instances capacity provider, IAM roles, task definition, service, and CloudWatch log group. It does not create the VPC, private subnets, security groups, ALB/listener/target group, ECR repository, Secrets Manager entries, control database, Vault, target databases, or admin UI. Deploy the UI separately using the [shared admin UI instructions](README.md#deploy-the-admin-ui).

## Before you start

Prepare these shared resources first:

- ECS Managed Instances enabled in a supported account/Region and an AWS deployment identity allowed to create the example resources and pass the infrastructure, instance, and task roles.
- At least two private subnets in one VPC. Provide existing Managed Instance and task security groups. Task ingress must allow TCP/8080 from the private ALB security group; egress must reach the control PostgreSQL database, Vault, target databases, OIDC discovery/JWKS, and approved telemetry. Configure the Managed Instance path to ECS control-plane, ECR, CloudWatch Logs, and task-secret APIs with VPC endpoints or approved NAT/proxy routes.
- A private ALB target group using `target_type = "ip"`, forwarding to port 8080 and health-checking `/readyz` for status `204`. The example does not change the listener or its rules.
- An existing private ECR repository and an image build host with Docker Buildx, authenticated to that registry. Build `linux/amd64` for the sample's `X86_64` architecture; for Graviton build `linux/arm64` and change the local architecture value to `ARM64`.
- Four Secrets Manager entries: the dedicated control-plane PostgreSQL URL, an at-least-32-byte `MCP_OAUTH_STATE_KEY`, Vault administration token, and separate restricted Vault runtime token. OpenTofu receives only their ARNs, never secret contents. Set `mcp_public_url` to the gateway's external HTTPS origin and allow `${mcp_public_url}/oauth/callback` in the public OIDC client's redirect URIs. Keep the state key stable across replicas/restarts. For customer-managed KMS keys, provide their ARNs and update each KMS key policy to permit the task execution role to decrypt.
- OIDC issuer/client/audience, an exact bootstrap administrator `sub`, an HTTPS Vault URL, and the separate admin UI deployment/routing. The control database role needs the migration rights used by the gateway at startup, limited to the dedicated control database.

The task execution role fetches task secrets and writes logs. The application task role has no AWS permissions. The Managed Instances instance role is named with the `ecsInstanceRole-` prefix for the managed policy's `iam:PassRole` scope. ECS Managed Instances have no SSH access and replace instances automatically; use ECS Exec/CloudWatch for operations.

## Deploy the gateway

### 1. Build and push an immutable image

Set these non-secret values in your deployment shell for the private ECR repository:

```sh
export AWS_REGION=us-east-1
export AWS_ACCOUNT_ID=123456789012
export ECR_REPOSITORY=sql-mcp-gateway
export RELEASE_TAG=replace-with-an-immutable-release-tag
```

Create the repository once if it is not already owned by your platform stack:

```sh
aws ecr create-repository --region "$AWS_REGION" --repository-name "$ECR_REPOSITORY" \
  --image-tag-mutability IMMUTABLE --encryption-configuration encryptionType=AES256
```

Authenticate to ECR and build/push from the repository root:

```sh
aws ecr get-login-password --region "$AWS_REGION" \
  | docker login --username AWS --password-stdin "$AWS_ACCOUNT_ID.dkr.ecr.$AWS_REGION.amazonaws.com"

docker buildx build --platform linux/amd64 \
  -f deploy/docker/gateway.Dockerfile \
  -t "$AWS_ACCOUNT_ID.dkr.ecr.$AWS_REGION.amazonaws.com/$ECR_REPOSITORY:$RELEASE_TAG" \
  --push .
```

Find the pushed digest:

```sh
aws ecr describe-images --region "$AWS_REGION" --repository-name "$ECR_REPOSITORY" \
  --image-ids imageTag="$RELEASE_TAG" --query 'imageDetails[0].imageDigest' --output text
```

Use the returned digest to form `image_uri` in the next step:
`<account>.dkr.ecr.<region>.amazonaws.com/<repository>@sha256:<digest>`. Do not deploy the mutable tag.

### 2. Set operator-local OpenTofu values

From the example directory:

```sh
cd examples/opentofu/ecs-managed-instances
cp locals.auto.tf.example locals.auto.tf
```

Edit `locals.auto.tf` and replace the example image URI, private subnet IDs, Managed Instance/task security groups, ALB target group ARN, secret ARNs, OIDC settings, bootstrap administrator subject, and Vault URL. Tune task CPU/memory, architecture, desired count, query bounds, storage, and log retention for your workload. The file is ignored by Git; never put resolved database/Vault credentials in it. Values here do include internal resource IDs and the OIDC subject, so do not publish a customized copy.

Before `tofu init`, configure your organization's encrypted, access-controlled, locked remote state backend in this example directory. The example intentionally does not choose a backend. Keep the generated `.terraform.lock.hcl` under version control for reproducible provider selection.

### 3. Format, initialize, validate, and inspect the plan

Run from `examples/opentofu/ecs-managed-instances/`:

```sh
tofu fmt -check -recursive
tofu init
tofu validate
tofu plan -out=reviewed.tfplan
```

Review every IAM action/resource, subnet and security-group route, role pass-through, Secrets Manager/KMS permission, expected instance/task count, compute/storage choice, CloudWatch retention, and cost. Confirm the image digest and target group are correct. The example's defaults use On-Demand compute, two tasks, and a rolling deployment circuit breaker; they are starting values, not sizing recommendations.

### 4. Deploy only after your plan is approved

The apply is an operator action, not an automatic step in this guide. After your change-control process approves the exact plan, run `tofu apply reviewed.tfplan` from the example directory. Keep the saved plan private and delete it through your normal secure artifact-retention process after use. OpenTofu can create IAM and ECS resources and incur AWS charges.

### 5. Verify the service and private route

Inspect the outputs and ECS service state:

```sh
tofu output
aws ecs describe-services --region "$AWS_REGION" \
  --cluster "$(tofu output -raw ecs_cluster_name)" \
  --services "$(tofu output -raw gateway_service_name)" \
  --query 'services[0].{status:status,running:runningCount,desired:desiredCount,rollout:deployments[*].rolloutState}' \
  --output table
```

From an approved VPN/private-network client, call the host routed by the private ALB:

```sh
curl -i https://sql-gateway.example.internal/health
curl -i https://sql-gateway.example.internal/readyz
```

Expect `204` from both. `/readyz` checks only the control PostgreSQL database. Review the task logs in the `/ecs/<name>/gateway` CloudWatch log group and verify the ALB target is healthy. Route `/mcp`, `/v1/*`, `/health`, and `/readyz` to the gateway; route remaining paths to a separately deployed admin UI on the same trusted origin. Register that UI's exact `/auth/callback` URL with OIDC. The example does not create the admin service or ALB listener rules.

Sign in as the bootstrap administrator, register a target through the UI, require each user to sign in once, grant minimum access, and test an authenticated MCP JSON-RPC `POST /mcp` request. The MCP transport is request/response JSON; this app does not implement Streamable HTTP SSE or GET sessions.

## Upgrade and rollback

Build and push each release under a new immutable tag, then pin `image_uri` to its new digest and review the OpenTofu plan. Keep the previous digest and task definition. If a deployment fails its ECS circuit breaker, inspect ECS service events and CloudWatch logs before retrying. To roll back an already completed release, restore the previous digest in `locals.auto.tf` and review/apply that plan. The gateway runs database migrations at process startup and allows up to 20 control-database connections per task; test schema compatibility and the rollout connection budget before production.

Complete the shared [production readiness checklist](../production-readiness.md).

## References

- [ECS Managed Instances capacity provider](https://docs.aws.amazon.com/AmazonECS/latest/developerguide/create-capacity-provider-managed-instances.html)
- [Managed Instances instance profile](https://docs.aws.amazon.com/AmazonECS/latest/developerguide/managed-instances-instance-profile.html)
- [Task execution role and secret injection](https://docs.aws.amazon.com/AmazonECS/latest/developerguide/task_execution_IAM_role.html)
- [AWS provider `aws_ecs_capacity_provider` schema](https://registry.terraform.io/providers/hashicorp/aws/6.30.0/docs/resources/ecs_capacity_provider)
