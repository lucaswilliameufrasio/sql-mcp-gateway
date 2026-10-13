# Amazon ECS Managed Instances

The OpenTofu example deploys the **gateway container only** to an ECS service
using Amazon ECS Managed Instances. It creates the cluster, Managed Instances
capacity provider, required IAM roles, task definition, service, and CloudWatch
log group. The VPC, private subnets, security groups, ALB/listener/target group,
Secrets Manager values, Vault, databases, and admin UI remain owned by your
existing platform stacks.

The capacity provider uses On-Demand compute, current-generation instance
requirements, detailed monitoring, and the ECS-managed instance profile. ECS
Managed Instances have no SSH access and automatically replace instances; plan
for that lifecycle and use ECS Exec/CloudWatch for operations.

## Prerequisites and network contract

- Enable/use ECS Managed Instances in a supported account/Region and allow the
  deployment identity to pass the infrastructure, instance, and task roles.
- Supply at least two private subnets in one VPC. Both the Managed Instance
  security groups and task security groups are inputs rather than created here.
- The task security group must accept TCP/8080 from the private ALB security
  group and allow egress from the application to the control PostgreSQL
  database, Vault, registered PostgreSQL/MySQL targets, OIDC discovery/JWKS,
  and approved telemetry. Separately configure the Managed Instance security
  group and endpoint security groups for ECS agent/control-plane traffic, ECR
  image pulls, CloudWatch Logs, and task-secret injection as required by the
  AWS Managed Instances network path. Use the required interface/gateway VPC
  endpoints or NAT/proxy routes for your Region and topology; do not expose the
  task or database to the public internet.
- The supplied ALB target group must be private, use `target_type = "ip"`,
  forward to container port 8080, and health-check `/readyz` with status `204`.
  Its listener/TLS policy and listener rules are intentionally not modified.
- Provision three Secrets Manager entries: the dedicated control database
  URL, the Vault administration token, and the separate restricted Vault
  runtime token. OpenTofu receives only their ARNs, never their values. Add
  customer-managed KMS key ARNs if those secrets use a CMK.
- If using a customer-managed KMS key, its key policy must also allow the task
  execution role to decrypt the referenced secrets; an IAM policy grant alone
  may not be sufficient.
- The OIDC issuer and Vault endpoint must use HTTPS. Verify that the exact
  bootstrap administrator subjects are correct before applying.

The task execution role fetches the referenced secrets and writes container
logs. The application task role has no AWS permissions. The instance role name
starts with `ecsInstanceRole-` to satisfy the managed policy's `iam:PassRole`
scope. The deployer still needs narrowly scoped permission to pass the
infrastructure, instance, and task roles to their respective AWS services.

## Build and publish the image

Build for the CPU architecture selected in `terraform.tfvars` and publish to a
private ECR repository. This sample's default architecture is x86-64:

```sh
docker buildx build --platform linux/amd64 \
  -f deploy/docker/gateway.Dockerfile \
  -t "$ECR_REPOSITORY_URI:$RELEASE_TAG" --push .
```

Set `image_uri` to the immutable ECR image URI containing its digest, rather
than a mutable `latest` tag. For Graviton, build `linux/arm64` and set
`cpu_architecture = "ARM64"`.

## Configure and review OpenTofu

Start in [`examples/opentofu/ecs-managed-instances/`](../../examples/opentofu/ecs-managed-instances/):

```sh
cp terraform.tfvars.example terraform.tfvars
```

Replace all sample VPC, target group, secret ARN, image digest, identity, and
Vault values. `terraform.tfvars` is an operator-local input file; do not commit
it if it contains internal identifiers. It must never contain secret values.
Use your team's encrypted, access-controlled, locked OpenTofu state backend;
review and commit the generated `.terraform.lock.hcl` for reproducible provider
selection.

From that directory, inspect formatting and provider/schema validation, then
review the plan:

```sh
tofu fmt -check -recursive
tofu init
tofu validate
tofu plan
```

The example does not run `tofu apply`; apply only after reviewing the plan,
account permissions, expected compute/cost, and your change-control process.

## Service and admin UI routing

The service exposes the gateway on port 8080 only. The target group's `/readyz`
check proves process/control-database readiness, not Vault or target database
availability. Configure the private ALB to send `/mcp`, `/v1/*`, `/health`, and
`/readyz` to this target group. Send the remaining paths to a separately
deployed SvelteKit Node-adapter admin service, normally on the same private ALB
and TLS origin. The example does not create that UI service or its listener
rules.

The gateway runs SQLx migrations on every process startup and currently allows
up to 20 control-database pool connections per task. Budget the RDS connection
limit for the task count, the admin UI, rollout surge, and migration activity.
The ECS service defaults to two tasks and a rolling circuit-breaker deployment;
validate database connection/migration behavior under that rollout before
production.

Use immutable task revisions, encrypted CloudWatch log retention, and alarms
for service/task health, error rate, dependency failures, lease issuance and
revocation, and database connectivity. Complete the shared
[production-readiness checklist](../production-readiness.md) before enabling
users.

## References

- [ECS Managed Instances capacity provider](https://docs.aws.amazon.com/AmazonECS/latest/developerguide/create-capacity-provider-managed-instances.html)
- [Managed Instances instance profile](https://docs.aws.amazon.com/AmazonECS/latest/developerguide/managed-instances-instance-profile.html)
- [Task execution role and secret injection](https://docs.aws.amazon.com/AmazonECS/latest/developerguide/task_execution_IAM_role.html)
- [AWS provider `aws_ecs_capacity_provider` schema](https://registry.terraform.io/providers/hashicorp/aws/6.30.0/docs/resources/ecs_capacity_provider)
