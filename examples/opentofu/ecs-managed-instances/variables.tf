variable "aws_region" {
  description = "AWS Region containing the VPC and ECS resources."
  type        = string
}

variable "name" {
  description = "Short, unique name prefix for this deployment."
  type        = string

  validation {
    condition     = can(regex("^[A-Za-z0-9_-]{3,32}$", var.name))
    error_message = "name must be 3-32 letters, digits, hyphens, or underscores."
  }
}

variable "environment" {
  description = "Environment tag, such as staging or production."
  type        = string
}

variable "image_uri" {
  description = "Published Linux container image URI; prefer an immutable ECR digest."
  type        = string
}

variable "private_subnet_ids" {
  description = "At least two private subnet IDs in the same VPC, across AZs."
  type        = list(string)

  validation {
    condition     = length(var.private_subnet_ids) >= 2
    error_message = "Provide private subnets in at least two Availability Zones."
  }
}

variable "managed_instance_security_group_ids" {
  description = "Existing security groups for the ECS Managed Instances network interfaces."
  type        = list(string)

  validation {
    condition     = length(var.managed_instance_security_group_ids) > 0
    error_message = "Provide at least one managed-instance security group."
  }
}

variable "task_security_group_ids" {
  description = "Existing task security groups; allow ingress from the private ALB on TCP/8080 and required egress."
  type        = list(string)

  validation {
    condition     = length(var.task_security_group_ids) > 0
    error_message = "Provide at least one task security group."
  }
}

variable "private_alb_target_group_arn" {
  description = "Existing private ALB target group ARN; it must use target_type = ip and health check /readyz."
  type        = string
}

variable "control_database_url_secret_arn" {
  description = "Secrets Manager ARN for the dedicated control-plane PostgreSQL URL."
  type        = string
}

variable "vault_admin_token_secret_arn" {
  description = "Secrets Manager ARN containing the Vault administration token."
  type        = string
}

variable "vault_runtime_token_secret_arn" {
  description = "Secrets Manager ARN containing the separate, restricted Vault runtime token."
  type        = string
}

variable "secret_kms_key_arns" {
  description = "Customer-managed KMS key ARNs used to encrypt the referenced Secrets Manager secrets; leave empty for the AWS-managed key."
  type        = list(string)
  default     = []
}

variable "oidc_issuer_url" {
  description = "HTTPS OIDC issuer URL reachable from the task subnets."
  type        = string
}

variable "oidc_client_id" {
  description = "OIDC browser client identifier."
  type        = string
}

variable "oidc_access_token_audience" {
  description = "Expected audience/resource claim for gateway access tokens."
  type        = string
}

variable "oidc_bootstrap_admin_subjects" {
  description = "Exact OIDC subject identifiers allowed to bootstrap as platform administrators."
  type        = list(string)
}

variable "vault_address" {
  description = "HTTPS Vault URL reachable from the task subnets."
  type        = string
}

variable "max_query_rows" {
  description = "Maximum query result rows (gateway accepts values from 1 through 1000)."
  type        = number
  default     = 200

  validation {
    condition     = var.max_query_rows >= 1 && var.max_query_rows <= 1000
    error_message = "max_query_rows must be between 1 and 1000."
  }
}

variable "max_query_duration_seconds" {
  description = "Maximum query duration (gateway accepts values from 1 through 300 seconds)."
  type        = number
  default     = 60

  validation {
    condition     = var.max_query_duration_seconds >= 1 && var.max_query_duration_seconds <= 300
    error_message = "max_query_duration_seconds must be between 1 and 300."
  }
}

variable "task_cpu" {
  description = "ECS task CPU units."
  type        = number
  default     = 512
}

variable "task_memory_mib" {
  description = "ECS task memory in MiB."
  type        = number
  default     = 1024
}

variable "cpu_architecture" {
  description = "Container architecture selected for the task definition."
  type        = string
  default     = "X86_64"

  validation {
    condition     = contains(["X86_64", "ARM64"], var.cpu_architecture)
    error_message = "cpu_architecture must be X86_64 or ARM64 and must match the published image."
  }
}

variable "instance_storage_size_gib" {
  description = "Root storage size for ECS Managed Instances."
  type        = number
  default     = 30
}

variable "desired_count" {
  description = "Number of gateway tasks. Use at least two for service-level redundancy."
  type        = number
  default     = 2

  validation {
    condition     = var.desired_count >= 1
    error_message = "desired_count must be at least 1."
  }
}

variable "log_retention_in_days" {
  description = "CloudWatch Logs retention period."
  type        = number
  default     = 30
}
