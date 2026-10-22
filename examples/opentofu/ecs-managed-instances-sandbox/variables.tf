variable "aws_region" {
  description = "AWS Region for this isolated, temporary sandbox."
  type        = string
  default     = "us-east-1"
}

variable "sandbox_suffix" {
  description = "Unique lowercase suffix used in every sandbox resource name."
  type        = string

  validation {
    condition     = can(regex("^[a-z0-9][a-z0-9-]{4,10}[a-z0-9]$", var.sandbox_suffix))
    error_message = "sandbox_suffix must be 6-12 lowercase letters, digits, or internal hyphens."
  }
}

variable "vpc_cidr" {
  description = "CIDR for the newly created, isolated sandbox VPC."
  type        = string
  default     = "10.87.0.0/16"

  validation {
    condition = (
      can(cidrnetmask(var.vpc_cidr)) &&
      can(cidrhost(var.vpc_cidr, 0)) &&
      try(tonumber(split("/", var.vpc_cidr)[1]) >= 16, false) &&
      try(tonumber(split("/", var.vpc_cidr)[1]) <= 20, false)
    )
    error_message = "vpc_cidr must be IPv4 with a prefix between /16 and /20 so the four subnets remain /24 through /28."
  }
}
