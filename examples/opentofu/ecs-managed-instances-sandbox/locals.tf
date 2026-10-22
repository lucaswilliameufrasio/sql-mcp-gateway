locals {
  name_prefix          = "sqlgw-${var.sandbox_suffix}"
  domain               = "${local.name_prefix}.internal"
  bootstrap_key_prefix = "bootstrap/${local.name_prefix}"
  ca_parameter_name    = "/${local.name_prefix}/tls/root-ca"
  availability_zones = slice(
    data.aws_availability_zones.available.names,
    0,
    2,
  )

  public_subnet_cidrs = [
    cidrsubnet(var.vpc_cidr, 8, 0),
    cidrsubnet(var.vpc_cidr, 8, 1),
  ]
  private_subnet_cidrs = [
    cidrsubnet(var.vpc_cidr, 8, 16),
    cidrsubnet(var.vpc_cidr, 8, 17),
  ]

  common_tags = {
    Application = "sql-mcp-gateway"
    Environment = "sandbox"
    ManagedBy   = "OpenTofu"
    SandboxId   = var.sandbox_suffix
  }

  database_secret_arns = [aws_db_instance.control.master_user_secret[0].secret_arn]

  gateway_secret_arns = [
    aws_secretsmanager_secret.control_database_url.arn,
    aws_secretsmanager_secret.vault_admin_token.arn,
    aws_secretsmanager_secret.vault_runtime_token.arn,
  ]

  bootstrap_config = {
    aws_region        = var.aws_region
    name_prefix       = local.name_prefix
    domain            = local.domain
    alb_dns_name      = aws_lb.gateway.dns_name
    admin_image       = "${aws_ecr_repository.admin.repository_url}:sandbox-${var.sandbox_suffix}"
    ca_parameter_name = local.ca_parameter_name
    gateway_secret_arns = {
      control_database_url = aws_secretsmanager_secret.control_database_url.arn
      vault_admin_token    = aws_secretsmanager_secret.vault_admin_token.arn
      vault_runtime_token  = aws_secretsmanager_secret.vault_runtime_token.arn
    }
    database_master_secret_arns = {
      control_postgres = aws_db_instance.control.master_user_secret[0].secret_arn
    }
    database_endpoints = {
      control_postgres = aws_db_instance.control.address
      target_postgres  = aws_db_instance.target_postgres.address
      target_mysql     = aws_db_instance.target_mysql.address
    }
    control_database_name = aws_db_instance.control.db_name
  }
}
