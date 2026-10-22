# Temporary ECS Managed Instances validation sandbox

Status: The temporary base and gateway sandboxes have been destroyed using separately reviewed, separately approved OpenTofu destroy plans: 76 base resources and 14 gateway resources, with no additions or changes. Both states contain zero managed resources and remain owner-only (`0600`). Read-only AWS checks found no sandbox RDS instances, VPC, active EC2 instances, EIP, network interface, EBS volume/snapshot, security group, subnet, route table, internet gateway, active NAT gateway, load balancer, ECR repository, Secrets Manager secret, S3 bucket, hosted zone, database snapshot, or gateway log group. One NAT gateway API tombstone is in `deleted` state. ECS cluster, service, and capacity-provider records are `INACTIVE` with zero running tasks. The pre-existing `ecs-mi-benchmark:14` task definition remains present as `INACTIVE`. OIDC/MCP end-to-end validation was skipped because no VPC-connected test client was available and the operator prioritized stopping ongoing costs. No secret values or raw logs were exposed.
Date: 2026-10-22

## Goal and safety boundary

Validate the SQL MCP Gateway on Amazon ECS Managed Instances, including OIDC,
Vault-issued PostgreSQL/MySQL credentials, and an authenticated MCP request. The
sandbox is temporary, isolated in a new VPC, and contains synthetic data only.
Every taggable AWS resource receives the sandbox's unique `SandboxId` tag, and
resource names use a unique prefix where supported. Untaggable sub-resources are
still scoped to this isolated OpenTofu state and reference only sandbox-owned
resources. Do not import, modify, or delete pre-existing resources. The inactive
`ecs-mi-benchmark:14` task-definition revision is outside this plan.

## Support-host bootstrap diagnosis

Read-only, bounded SSM diagnostics on 2026-10-22 found that cloud-init's
`scripts-user` stage failed while running the support bootstrap. The wrapper had
installed AWS CLI and downloaded the executable bootstrap script; the main
script created its compose directory but stopped before Docker installation or
Compose-file download. A filtered scan classified the DNF failure as conflicting
requests associated with the requested `curl` package. The host had
`curl-minimal` installed, the full `curl` package absent, and the existing curl
binary supports HTTPS. The bootstrap now requests `curl-minimal`, preserving the
HTTPS downloads without asking DNF to replace the preinstalled conflicting
package.

The first fresh, non-targeted plan after this correction proposed eight
in-place updates: the bootstrap S3 object, seven security groups, and the MySQL
parameter group; it proposed no additions or destructions. Those unrelated
security-group and parameter-group changes are not part of this repair and were
not applied. A separately saved targeted plan contained only an in-place update
to `aws_s3_object.support_bootstrap` (zero additions, one change, zero
destructions), and that exact plan was approved and applied. The target was
deliberate to prevent the unrelated rule and parameter diffs from being applied
while their cause remains unresolved. It added no hourly-billed resources; the
already-running base sandbox remains about US$0.2946/hour before variable usage.
This one-object update did not rerun EC2 user data; at the time of apply, the
host still needed to re-download the updated object and execute it via SSM.

The first explicit-region SSM replay advanced past package installation,
downloaded the Compose/configuration/TLS materials, and started the five
support containers. Authentik's worker stayed unhealthy and the Compose wait
failed; the other four services were running and healthy (Caddy has no
container-level healthcheck). Bounded diagnostics confirmed that the mounted
blueprint matched the versioned source and was mounted read-only, but its host
mode was `0600`. Authentik has non-root worker processes, and the file was not
readable as the worker user; filtered worker errors classified the permission
failure as associated with the blueprint mount. The worker's OIDC discovery
healthcheck returned 404. At that point the bootstrap completion marker and
Vault/database runtime artifacts were absent, so the gateway root was blocked.

The local retry fix changes the public, credential-free blueprint to mode
`0644` before Compose starts and restarts an existing Authentik worker on a
retry. It preserves an existing `.env` and complete CA/leaf TLS material, and
fails closed if the environment token is malformed or TLS material is
partial. Shell regression/syntax checks and all repository quality checks pass.
The reviewed saved plan was
`.dev/ecs-mi-sandbox-artifacts.ts7dEz/support-bootstrap-blueprint-retry.tfplan`;
it contains exactly one in-place update to `aws_s3_object.support_bootstrap`,
with zero additions and zero destructions. It adds no hourly-billed resources;
only a small S3 object update/request is expected, while the existing base
sandbox continues at about US$0.2946/hour before variable usage. The approved
plan applied successfully, and a read-only S3 metadata check confirmed the
object's content length matches the planned content. After separate
authorization, the SSM resume downloaded the object and verified its SHA-256
before execution. The bootstrap script returned successfully and created its
intentionally empty completion marker; the wrapper mistakenly treated that
empty file as missing and reported a failed invocation. A follow-up read-only
check confirmed the marker exists, `.env` remains private, all TLS materials
are present, the SSM CA parameter matches the local CA, Vault's runtime
environment exists, temporary secret files were removed, all five Compose
services are running, and the Authentik worker is healthy. No secret values or
raw logs were read or returned; the support bootstrap is complete.

Only fixed classifications, package names from the bootstrap allowlist, and
file-presence booleans were returned; no raw log lines, `.env` contents, or
credentials were read or exposed. The bootstrap script is stored as an S3
object, while EC2 user data refers to its stable bucket/key; changing that
object alone does not rerun user data. This sandbox host has now fetched and
executed the updated object through SSM. The bootstrap completion marker and
public CA were available, so the gateway root proceeded to its independent plan
and cost review. Separate apply approval was later obtained for the exact
no-placement plan and applied successfully, as recorded below.

The base-root plan was separately reviewed and authorized; its apply completed
with 76 resources added and no changes or deletions. The gateway root's initial
approved apply partially completed; its recovery and successful service apply
are described below.

## Gateway apply partial failure and recovery

The first separately approved gateway plan partially applied 12 managed
resources, then AWS rejected `CreateCapacityProvider` because the request
combined `instanceGenerations` with `allowedInstanceTypes`. That filter conflict
was removed while retaining the explicit `c7i.large` allowlist. The next
separately approved resume plan created and tracked the capacity provider, but
failed before creating the service. At that point, state had 13 managed
resources plus two data sources and remained owner-only (`0600`). Read-only AWS
checks then showed an active cluster, an `ACTIVE` Managed Instances capacity
provider associated with that cluster, the registered task definition, IAM
roles, and log group; no ECS service or running gateway tasks existed. No
gateway tasks or Managed Instances had yet been started, and no resources had
been cleaned up.

The second failure is attributable to managing the cluster association twice.
The AWS `CreateCapacityProvider` API accepts the target `cluster`; for Managed
Instances, AWS documents that the provider is available only in that specified
cluster. By contrast, AWS explicitly documents that
`PutClusterCapacityProviders` is not supported for Managed Instances, and the
AWS provider's `aws_ecs_cluster_capacity_providers` resource calls that API.
The prior config therefore attempted an unsupported separate association even
though the capacity provider was already associated. The config now removes
that resource and its service dependency; the ECS service's capacity-provider
strategy directly references the already-associated provider.

The prior saved plan,
`.dev/ecs-mi-sandbox-artifacts.ts7dEz/gateway-resume-20261022.tfplan`, is stale
after the partial apply and configuration fix; do not retry it or either older
gateway plan. The later service-only plan also failed during `CreateService`:
CloudTrail records `InvalidParameterException`, with the safe error fingerprint
matching a capacity-provider/placement incompatibility. AWS documents that ECS
Managed Instances does not support task placement strategies; it attempts to
spread tasks across accessible Availability Zones automatically. The explicit
`ordered_placement_strategy` block was therefore removed. The failed call left
no service or tasks; at that point, the 13-resource state and active, associated
capacity provider remained. That saved plan is stale and must not be retried.
The subsequent no-placement plan
`.dev/ecs-mi-sandbox-artifacts.ts7dEz/gateway-no-placement-20261022.tfplan`
(SHA-256 `dab276744c33d25c40e6bd6bc19b0f631845e6680fecf41d44318725f94ea1a6`)
proposed exactly one create (`aws_ecs_service.gateway`) and no updates,
deletions, or replacements. It uses the active, already-associated capacity
provider, two desired tasks, two private subnets with public IP assignment
disabled, and the existing IP target group on port 8080. It has no explicit
placement strategy; ECS Managed Instances attempts to spread tasks across
accessible AZs automatically. The exact plan was separately approved and
applied successfully: one resource added, zero changed, zero destroyed. At that
point, the gateway state tracked 14 managed resources. A subsequent read-only
check confirmed the service `ACTIVE`, deployment `COMPLETED` (2 desired, 2
running, 0 pending), two tasks `RUNNING`, and two registered ALB targets both
`healthy`; the capacity provider was `ACTIVE` with `CREATE_COMPLETE`. The
separate gateway destroy and final cleanup results are recorded below. The
pre-cleanup estimate was about US$0.2065/hour incremental and US$0.5011/hour
combined, before variable usage.

## Sandbox topology

- **Network:** one new VPC, two private subnets in separate AZs, public subnets
  only for one NAT Gateway, and private route tables. Use one NAT to minimize
  cost; this is not a highly available production topology. No public inbound
  rules or SSH access. Manage the support EC2 host through Systems Manager.
- **Ingress:** Caddy on the private support host terminates test TLS for the
  private Authentik, Vault, and gateway/UI hostnames. It proxies gateway traffic
  to an HTTP-only internal ALB, whose listener accepts traffic only from that
  support host. ECS tasks can reach only the support host on HTTPS and the
  databases on their required ports. Vault's database plugins also need
  support-host-to-target access on TCP/5432 and TCP/3306; security-group
  references restrict this to Vault's host and the two database ports.
- **Gateway:** two ECS tasks spread across the two AZs, an internal ALB, and an
  ECS Managed Instances capacity provider restricted to `c7i.large` On-Demand
  instances. The example's general instance requirements allow 2–16 vCPU and
  4–32 GiB; this sandbox's `allowed_instance_types` narrows eligible capacity
  to `c7i.large`. Keep the task role without AWS permissions and scope the
  execution role to its three secret ARNs and its log group.
- **Databases:** three Single-AZ `db.t4g.small` RDS instances with 20 GB gp3
  each: a dedicated control-plane PostgreSQL database, a PostgreSQL target, and
  a MySQL target. Use throwaway records. Do not use the control database as a
  registered target.
- **Support host:** one private `t3.large` EC2 instance with a 20 GB encrypted
  gp3 volume, SSM access only, and a narrowly scoped instance profile. Run the
  test Authentik OIDC provider, its disposable PostgreSQL database, Vault in
  development mode, Caddy, and the separately built admin UI. The host can read
  only the control database's RDS-managed master secret; it creates a separate
  gateway database role and stores the URL in the sandbox secret. It has no IAM
  permission to read either target's master secret; target provisioner
  credentials were intended to be submitted through the private admin
  registration flow and stored by Vault. The temporary support stack has since
  been destroyed; full validation did not complete.
- **TLS:** issue a test-only private CA and leaf certificate on the support
  host. Use HTTPS for the remote OIDC issuer and Vault endpoints, and add only
  the public CA certificate to a sandbox-only gateway image. Do not use the
  local Compose stack's loopback HTTP settings in ECS. Do not create an AWS
  Private CA.
- **Image and secrets:** use a new private ECR repository with immutable image
  digest for both the gateway and admin UI. Create only the three Secrets
  Manager entries required by the ECS example; RDS also creates three
  master-user secrets that it manages. Keep secret values out of OpenTofu
  inputs, state, logs, and output; the host/task roles use secret ARNs.
  RDS-managed master passwords remain AWS-managed. A private,
  SSE-S3-encrypted bootstrap bucket temporarily holds only non-secret scripts,
  configuration, and the Compose/Authentik blueprint; block all public access
  and force-delete only this sandbox bucket during teardown.
- **Logs:** create a dedicated gateway log group with one-day retention. No
  account-wide logging, registry scanning, or other shared settings are
  changed.

Prepared artifacts:

- `examples/opentofu/ecs-managed-instances-sandbox/` creates the isolated base
  resources and bootstraps temporary support services.
- `examples/opentofu/ecs-managed-instances/` remains the gateway-only root; its
  sandbox capacity provider is restricted to `c7i.large` through
  `allowed_instance_types`.
- `deploy/docker/gateway.ecs-mi-sandbox.Dockerfile` adds only the public test
  CA and public RDS CA bundle to the gateway trust store. Build the admin image
  with `apps/admin/Dockerfile.sandbox`.
- `scripts/render-ecs-mi-sandbox-gateway-locals.py` creates the ignored,
  owner-only gateway `locals.auto.tf` from base-root outputs and an immutable
  image digest. Invoke it with `python3` and the `sha256:<64-hex-digest>`; it
  refuses to overwrite an existing file.
- `scripts/prepare-ecs-mi-sandbox-build-context.sh` retrieves only the public
  test CA from Parameter Store and the public RDS CA bundle for the gateway
  image; invoke it with `sh`, the region, and the CA parameter name. It refuses
  to overwrite existing build-context files.
- `scripts/generate-ecs-mi-sandbox-deploy-policy.sh` runs IAM Policy Autopilot
  on a saved plan JSON and writes an owner-only policy file. Invoke it with `sh`
  and absolute input/output paths, with `AWS_REGION` and `AWS_ACCOUNT_ID` set.
  It never uploads or applies the policy.

The support host's target-database route is deliberately limited to the two
target security groups and their native database ports. This is required
because Vault—not the gateway container—opens database connections to create,
rotate, and revoke dynamic database users. The support instance role still has
no permission to read the target RDS master-secret ARNs.

The public test CA is represented by an OpenTofu-managed SSM parameter. Bootstrap
updates its value after generating the CA; `ignore_changes` keeps that external
public-certificate update from causing drift, while the resource remains in
state for teardown. AWS resources that support tags receive `SandboxId`; AWS
sub-resources without tag APIs are removed through their owning tracked parent.

The support bootstrap uses a versioned Docker Compose download URL and
version-tagged container images, but does not yet verify a pinned binary
checksum or container image manifest digests. Review those upstream artifact
identities before applying if reproducible supply-chain inputs are required.

The roots required **two review/approval gates** because the second plan depended
on generated resource IDs, bootstrapped Secrets Manager values, and the pushed
image digest. Both gates completed: the base root was applied, and the gateway's
exact no-placement plan was separately reviewed, approved, and applied. When a
VPC-connected test client was unavailable, the operator chose to stop ongoing
costs; separately approved destroy plans removed the gateway root first, then
the base root. OIDC/MCP end-to-end validation remains incomplete; no public
ingress was added.

## Validation and lifecycle

1. **Completed:** validated both OpenTofu roots. Keep state, saved plans, plan
   JSON, and generated locals on encrypted storage with owner-only permissions;
   never print the full plan JSON.
2. **Completed:** reviewed the resource action lists, IAM policy, selected ECS
   instance type, routes, secret references, database deletion settings, and
   costs before the respective applies.
3. **Base stage completed:** separately approved and applied the base-root plan
   (76 resources added, none changed or destroyed); verified the SSM managed-node
   state and bootstrap completion marker, retrieved public CA material, built
   both images, and pushed them by immutable tags/digests.
4. **Gateway stage completed:** generated the gateway root locals from base
   outputs and the pinned image digest, reviewed the no-placement plan, received
   its separate approval, and applied that exact plan successfully. The saved
   plan hash and result are recorded above.
5. **End-to-end validation skipped:** no VPC-connected browser or test client
   was available for OIDC, database target registration, authenticated MCP
   requests, and Vault lease-revocation tests. The operator chose teardown rather
   than continue incurring the sandbox's hourly cost. The admin Compose profile
   was not started as part of this skipped validation stage.
6. **Cleanup completed:** after separate review and approval of each exact plan,
   destroyed the gateway root first (14 resources), then generated a fresh base
   destroy plan and destroyed the base root (76 resources). Both applies had
   zero additions and zero changes. The base plan deleted all three RDS
   instances without final snapshots; the data was synthetic and disposable.
7. **Independent readback completed:** both OpenTofu states have zero managed
   resources. AWS checks found no sandbox RDS instances or snapshots, VPC, active
   EC2 instances, EIP, network interfaces, EBS volumes or snapshots, security
   groups, subnets, route tables, internet gateway, active NAT gateway, load
   balancer, ECR repositories, secrets, S3 bucket, hosted zone, or gateway log
   group. One NAT tombstone is in `deleted` state. ECS objects are inactive
   tombstones with zero running tasks. The pre-existing `ecs-mi-benchmark:14`
   remains present and `INACTIVE`.
8. No cleanup action targeted resources outside these two sandbox states. No
   credentials, secret values, or raw logs were read or returned.

## Operator and IAM review

- The user authorized the currently logged-in root session for this sandbox;
  the saved base plan confirms that the local OpenTofu caller is root in the
  same account as the AWS MCP session. No temporary IAM role has been created.
  Apply approval is specific to an exact saved plan. The earlier
  trust-hardened and resume-plan approvals were used, but those applies only
  partially completed. A separate approval was subsequently obtained for the
  exact no-placement plan; that approved apply completed successfully.
- The base plan was analyzed locally with `iam-policy-autopilot`. Its output is
  a review baseline only: three policy documents, 234 unique actions across 12
  service namespaces, and nine wildcard-resource warnings. A rerun with service
  hints was identical. Raising `--resource-cutoff` to 100 did not narrow the
  wildcard `iam:PassRole` statements.
- Four base-plan `iam:PassRole` statements use `Resource: "*"`, constrained by
  `iam:PassedToService` to RDS (including `rds-preview`), EC2, or S3; one
  additional `PassRole` statement has one exact resource. Autopilot's
  explanation includes optional RDS replica/modify/restore and S3 replication
  operations as well as the planned instance-profile/EC2 operations. The
  wildcard statements are not acceptable as a least-privilege deployment
  policy. The baseline also includes IAM tagging permissions for users, MFA
  devices, OIDC/SAML providers, and server certificates, which are not sandbox
  resources and need review before any policy is used.
- No generated policy was uploaded, attached, or applied. Do not use this
  baseline as an operator policy. Any later deployment policy must be reviewed
  and scoped to sandbox-created roles and resources. Image-push permissions for
  the two sandbox ECR repositories and SSM session/command permissions for the
  one support instance are separate and are not implied by the base plan.
- The old `gateway-plan-20261022.tfplan` predates trust-policy hardening. The
  approved `gateway-plan-trust-20261022.tfplan` partially applied and is stale.
  The approved `gateway-resume-20261022.tfplan` also partially applied and is
  stale. Do not apply any of these earlier plans again.
- The failed service-only plan
  `.dev/ecs-mi-sandbox-artifacts.ts7dEz/gateway-service-only-20261022.tfplan`
  created only `aws_ecs_service.gateway`, but AWS rejected its explicit
  placement strategy. It is stale after the failed apply and subsequent
  configuration change; do not apply it again.
- The saved no-placement plan was
  `.dev/ecs-mi-sandbox-artifacts.ts7dEz/gateway-no-placement-20261022.tfplan`
  (SHA-256
  `dab276744c33d25c40e6bd6bc19b0f631845e6680fecf41d44318725f94ea1a6`): exactly
  one create (`aws_ecs_service.gateway`), with zero updates, deletions, or
  replacements. It uses the active capacity provider with a matching strategy,
  two desired tasks, two private subnets, public IPs disabled, the existing IP
  target group on port 8080, and no explicit task placement strategy. The
  digest-pinned task definition and three secret references were unchanged. The
  exact plan was separately approved and applied: 1 added, 0 changed, 0
  destroyed; at that point, state tracked 14 managed resources. Do not apply the
  saved plan again. Any future apply requires review and separate approval of a
  new exact plan.
- The no-placement plan's Autopilot baseline has five statements and 28 unique
  actions, with three wildcard-resource statements. Its one `iam:PassRole`
  statement is service-conditioned but retains a wildcard resource. This is not
  a least-privilege operator policy; it was not uploaded or applied. The plan's
  approval and successful apply did not authorize or require using that policy.

## Read-only AWS feasibility checks

Checked in `us-east-1` on 2026-10-22; these calls did not create or modify
resources:

- RDS reports `db.t4g.small` orderable with encrypted gp3 storage for PostgreSQL
  `17.6` and MySQL `8.4.7`, in `us-east-1a`, `1b`, `1c`, `1d`, and `1f`.
  EC2 reports all six standard AZ names as available. Before apply, both selected
  subnet AZs were confirmed against the RDS orderable set; `1e` was not in the
  returned RDS options. Orderability does not guarantee future physical capacity.
- The applied EC2 On-Demand Standard vCPU quota is `1,280`. The AWS/Usage metric
  returned only two samples over the prior 24 hours, with a maximum of 4 vCPUs,
  so treat that usage figure as indicative rather than a live inventory count.
  The proposed support host plus two `c7i.large` Managed Instances require 6
  additional vCPUs.
- The applied RDS DB-instance quota is 40; a metadata-only count returned zero
  DB instances in this Region. The planned three instances fit that quota, but
  this does not guarantee launch capacity. AWS documentation states that ECS
  Managed Instances use EC2 quotas; the ECS service quota results were API
  throttling limits, not a separate Managed Instances compute quota.

At apply time, the active, associated Managed Instances capacity provider
confirmed feature and Region availability for this account. The approved
deployment launched both tasks and both targets became healthy. This confirmed
capacity for that service count, not for future launches or replacements.

## Cost estimate

Illustrative On-Demand rates for `us-east-1`, with low traffic. Rates were
checked against the AWS Price List API on 2026-10-22; monthly storage and secret
charges are normalized using 730 hours/month:

| Component | State | Assumption | Approx. USD/hour |
| --- | --- | --- | ---: |
| Internal ALB | Destroyed | Base hourly rate plus about 1 LCU | 0.03050 |
| NAT Gateway | Destroyed | One gateway, before data processing | 0.04500 |
| NAT public IPv4 | Destroyed | One in-use public IPv4 address | 0.00500 |
| RDS | Destroyed | 3 × `db.t4g.small`, 20 GB gp3 each | 0.10545 |
| Support EC2 | Destroyed | `t3.large` plus 20 GB gp3 | 0.08539 |
| Secrets Manager | Destroyed | 6 secrets (3 sandbox entries and 3 RDS-managed master secrets) | 0.00329 |
| Small-usage allowance | Pre-cleanup estimate | ECR storage, logs, and light traffic | 0.02000 |
| **Base-only estimate** | **Before cleanup** |  | **about 0.29463** |
| ECS Managed Instances | Destroyed | 2 × `c7i.large`, MI fee, and 30 GB gp3 each | 0.20651 |
| **Combined planning total** | **Before cleanup** |  | **about 0.50114** |

The Price List rates used were: EC2 `c7i.large` $0.08925/hour plus the ECS
Managed Instances fee $0.010716/hour per instance; `t3.large` $0.0832/hour;
RDS `db.t4g.small` $0.032/hour; EC2 gp3 $0.08/GB-month; RDS gp3
$0.115/GB-month; ALB $0.0225/hour plus $0.008/LCU-hour; NAT $0.045/hour plus
$0.045/GB processed; public IPv4 $0.005/hour; and Secrets Manager $0.40 per
secret-month. Before cleanup, the base was estimated at **US$0.2946/hour** and
the gateway added **US$0.2065/hour**, for a combined estimate of
**US$0.5011/hour**.
Combined cost is about **US$0.5011 for one hour**, **US$1.0023 for two**, and
**US$2.0046 for four**, before variable NAT data processing, ALB use above one
LCU, cross-AZ transfer, and any usage beyond the small-usage allowance. This is
not a hard cap. The sandbox has now been destroyed; these figures are historical
pre-cleanup estimates, not current run-rate. Reprice before any future apply if
scope or rates have changed.

## Cleanup outcome and validation limitation

- The separately reviewed and approved gateway destroy plan,
  `.dev/ecs-mi-sandbox-artifacts.ts7dEz/gateway-destroy-20261022.tfplan`
  (SHA-256
  `e9cb6cb4d8f7a7a917759f88a16955c9e2212ac60b425a06e4ad6888d20f2fee`),
  destroyed 14 resources with zero additions or changes; do not reuse it.
- The earlier base destroy plan created before gateway teardown,
  `.dev/ecs-mi-sandbox-artifacts.ts7dEz/base-destroy-20261022.tfplan`, was not
  applied and is stale. Do not reuse it.
- After the gateway teardown, a fresh base destroy plan,
  `.dev/ecs-mi-sandbox-artifacts.ts7dEz/base-destroy-post-gateway-20261022.tfplan`
  (SHA-256
  `e96f8003c5469379c7bc95f8128ff1d365bde59b30b892635c65310eaddb77d9`),
  was separately reviewed and approved. It destroyed 76 state-managed resources
  with zero additions or changes, including three RDS instances with
  `skip_final_snapshot = true`; do not reuse the applied plan.
- Both OpenTofu states now list zero managed resources and remain mode `0600`.
  AWS readback confirmed no sandbox RDS instances, VPC, nonterminated EC2
  instances, EIP, network interfaces, EBS volumes or snapshots, security groups,
  subnets, route tables, internet gateway, active NAT gateway, load balancer,
  ECR repositories, secrets, S3 bucket, hosted zone, database snapshots, or
  gateway log group. The NAT gateway tombstone is `deleted`. ECS
  cluster/service/capacity-provider records are `INACTIVE`, with zero running
  tasks.
- The pre-existing `ecs-mi-benchmark:14` task definition remains present as
  `INACTIVE`, revision 14. It was absent from both destroy plans.
- OIDC, database target registration, authenticated MCP, and Vault lease
  revocation tests were not completed because no VPC-connected client was
  available; teardown was chosen to stop ongoing cost. No secret values or raw
  logs were retrieved or disclosed.
- Any future validation requires rebuilding the sandbox and separately
  reviewing and approving new plans. Do not reuse these applied plans.
- Local state files, backups, and saved plans were retained for audit. All 32
  files are mode `0600`; the ignored artifact directory is mode `0700`. These
  files may contain sensitive state and must not be shared.
