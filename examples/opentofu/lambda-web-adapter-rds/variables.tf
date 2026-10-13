variable "aws_region" {
  description = "AWS Region containing the VPC, RDS, Secrets Manager, and API Gateway resources."
  type        = string
}

variable "name" {
  description = "Short, unique Lambda function name."
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

variable "lambda_image_uri" {
  description = "Published Lambda container image URI; prefer an immutable ECR digest."
  type        = string
}

variable "private_subnet_ids" {
  description = "At least two private subnet IDs in the VPC containing RDS and Vault routes."
  type        = list(string)

  validation {
    condition     = length(var.private_subnet_ids) >= 2
    error_message = "Provide private subnets in at least two Availability Zones."
  }
}

variable "lambda_security_group_ids" {
  description = "Existing Lambda security groups with egress to Secrets Manager, OIDC, Vault, RDS, and approved target databases."
  type        = list(string)

  validation {
    condition     = length(var.lambda_security_group_ids) > 0
    error_message = "Provide at least one Lambda security group."
  }
}

variable "execute_api_vpc_endpoint_id" {
  description = "Existing execute-api interface VPC endpoint used by the private REST API."
  type        = string
}

variable "control_database_url_secret_arn" {
  description = "Secrets Manager ARN whose SecretString is the gateway control-plane PostgreSQL URL."
  type        = string
}

variable "vault_credentials_secret_arn" {
  description = "Secrets Manager ARN whose SecretString is JSON with admin_token and runtime_token fields."
  type        = string
}

variable "secret_kms_key_arns" {
  description = "Customer-managed KMS key ARNs used to encrypt the referenced secrets; leave empty for the AWS-managed key."
  type        = list(string)
  default     = []
}

variable "oidc_issuer_url" {
  description = "HTTPS OIDC issuer URL reachable from the Lambda subnets."
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

  validation {
    condition     = length(var.oidc_bootstrap_admin_subjects) > 0
    error_message = "Provide at least one verified OIDC subject for initial administration."
  }
}

variable "vault_address" {
  description = "HTTPS Vault URL reachable from the Lambda subnets."
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
  description = "Maximum database query duration (gateway accepts values from 1 through 300 seconds)."
  type        = number
  default     = 18

  validation {
    condition     = var.max_query_duration_seconds >= 1 && var.max_query_duration_seconds <= 300
    error_message = "max_query_duration_seconds must be between 1 and 300."
  }
}

variable "lambda_memory_mb" {
  description = "Lambda memory in MiB. Tune with representative requests."
  type        = number
  default     = 1024

  validation {
    condition     = var.lambda_memory_mb >= 128 && var.lambda_memory_mb <= 10240
    error_message = "lambda_memory_mb must be between 128 and 10240."
  }
}

variable "lambda_timeout_seconds" {
  description = "Lambda timeout; this example stays below the default private REST API integration timeout."
  type        = number
  default     = 28

  validation {
    condition     = var.lambda_timeout_seconds >= 1 && var.lambda_timeout_seconds <= 29
    error_message = "lambda_timeout_seconds must be between 1 and 29 for this API Gateway example."
  }
}

variable "reserved_concurrent_executions" {
  description = "Hard cap on Lambda execution environments; the gateway currently permits up to 20 control-DB pool connections per environment."
  type        = number
  default     = 5

  validation {
    condition     = var.reserved_concurrent_executions >= 1
    error_message = "reserved_concurrent_executions must be at least 1."
  }
}

variable "api_stage_name" {
  description = "Private API Gateway stage name."
  type        = string
  default     = "prod"
}

variable "api_rate_limit_per_second" {
  description = "API Gateway stage steady-state request rate limit."
  type        = number
  default     = 5
}

variable "api_burst_limit" {
  description = "API Gateway stage burst request limit."
  type        = number
  default     = 10
}

variable "log_retention_in_days" {
  description = "CloudWatch Logs retention period."
  type        = number
  default     = 30
}
