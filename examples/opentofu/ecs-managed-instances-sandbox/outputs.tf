output "aws_region" {
  description = "Region for the sandbox resources."
  value       = var.aws_region
}

output "sandbox_id" {
  description = "Unique sandbox suffix used to tag gateway-root resources."
  value       = var.sandbox_suffix
}

output "gateway_ecr_repository_url" {
  description = "Private ECR repository URL for the gateway image."
  value       = aws_ecr_repository.gateway.repository_url
}

output "admin_ecr_repository_url" {
  description = "Private ECR repository URL for the admin UI image."
  value       = aws_ecr_repository.admin.repository_url
}

output "gateway_name" {
  description = "Name to use in the existing ECS Managed Instances example."
  value       = "${local.name_prefix}-gateway"
}

output "admin_image_tag" {
  description = "Unique immutable tag for the sandbox admin UI image."
  value       = "${aws_ecr_repository.admin.repository_url}:sandbox-${var.sandbox_suffix}"
}

output "gateway_secret_arns" {
  description = "Secret ARNs populated by the support-host bootstrap; no values are output."
  value = {
    control_database_url = aws_secretsmanager_secret.control_database_url.arn
    vault_admin_token    = aws_secretsmanager_secret.vault_admin_token.arn
    vault_runtime_token  = aws_secretsmanager_secret.vault_runtime_token.arn
  }
}

output "gateway_target_group_arn" {
  description = "Internal ALB target group for the gateway-only ECS example."
  value       = aws_lb_target_group.gateway.arn
}

output "managed_instance_security_group_id" {
  value = aws_security_group.managed_instances.id
}

output "private_subnet_ids" {
  value = aws_subnet.private[*].id
}

output "private_test_urls" {
  description = "Private HTTPS origins; resolvable only from the sandbox VPC."
  value = {
    auth    = "https://auth.${local.domain}"
    vault   = "https://vault.${local.domain}:443"
    gateway = "https://gateway.${local.domain}"
  }
}

output "sandbox_domain" {
  value = local.domain
}

output "support_instance_id" {
  value = aws_instance.support.id
}

output "support_private_ip" {
  value = aws_instance.support.private_ip
}

output "support_security_group_id" {
  value = aws_security_group.support.id
}

output "task_security_group_id" {
  value = aws_security_group.tasks.id
}

output "target_database_endpoints" {
  description = "Private endpoints for the synthetic PostgreSQL and MySQL targets."
  value = {
    postgres = aws_db_instance.target_postgres.address
    mysql    = aws_db_instance.target_mysql.address
  }
}

output "target_database_master_secret_arns" {
  description = "RDS-managed secret ARNs; credentials are never output."
  value = {
    postgres = aws_db_instance.target_postgres.master_user_secret[0].secret_arn
    mysql    = aws_db_instance.target_mysql.master_user_secret[0].secret_arn
  }
}

output "tls_ca_parameter_name" {
  description = "SSM Parameter Store path containing the public test CA certificate."
  value       = local.ca_parameter_name
}

output "vpc_id" {
  value = aws_vpc.sandbox.id
}
