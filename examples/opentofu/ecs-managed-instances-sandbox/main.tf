resource "aws_vpc" "sandbox" {
  cidr_block           = var.vpc_cidr
  enable_dns_support   = true
  enable_dns_hostnames = true

  tags = {
    Name = local.name_prefix
  }
}

resource "aws_internet_gateway" "sandbox" {
  vpc_id = aws_vpc.sandbox.id

  tags = {
    Name = "${local.name_prefix}-igw"
  }
}

resource "aws_subnet" "public" {
  count = 2

  vpc_id                  = aws_vpc.sandbox.id
  availability_zone       = local.availability_zones[count.index]
  cidr_block              = local.public_subnet_cidrs[count.index]
  map_public_ip_on_launch = false

  tags = {
    Name = "${local.name_prefix}-public-${count.index + 1}"
    Tier = "public-egress"
  }
}

resource "aws_subnet" "private" {
  count = 2

  vpc_id                  = aws_vpc.sandbox.id
  availability_zone       = local.availability_zones[count.index]
  cidr_block              = local.private_subnet_cidrs[count.index]
  map_public_ip_on_launch = false

  tags = {
    Name = "${local.name_prefix}-private-${count.index + 1}"
    Tier = "private"
  }
}

resource "aws_route_table" "public" {
  vpc_id = aws_vpc.sandbox.id

  route {
    cidr_block = "0.0.0.0/0"
    gateway_id = aws_internet_gateway.sandbox.id
  }

  tags = {
    Name = "${local.name_prefix}-public"
  }
}

resource "aws_route_table_association" "public" {
  count = 2

  subnet_id      = aws_subnet.public[count.index].id
  route_table_id = aws_route_table.public.id
}

resource "aws_eip" "nat" {
  domain = "vpc"

  tags = {
    Name = "${local.name_prefix}-nat"
  }
}

resource "aws_nat_gateway" "sandbox" {
  allocation_id = aws_eip.nat.id
  subnet_id     = aws_subnet.public[0].id

  depends_on = [aws_internet_gateway.sandbox]

  tags = {
    Name = "${local.name_prefix}-nat"
  }
}

resource "aws_route_table" "private" {
  vpc_id = aws_vpc.sandbox.id

  route {
    cidr_block     = "0.0.0.0/0"
    nat_gateway_id = aws_nat_gateway.sandbox.id
  }

  tags = {
    Name = "${local.name_prefix}-private"
  }
}

resource "aws_route_table_association" "private" {
  count = 2

  subnet_id      = aws_subnet.private[count.index].id
  route_table_id = aws_route_table.private.id
}

resource "aws_security_group" "managed_instances" {
  name        = "${local.name_prefix}-ecs-mi"
  description = "ECS Managed Instances; no inbound rules."
  vpc_id      = aws_vpc.sandbox.id
  ingress     = []
  egress      = []
}

resource "aws_security_group" "tasks" {
  name        = "${local.name_prefix}-ecs-tasks"
  description = "Gateway tasks; ALB ingress only."
  vpc_id      = aws_vpc.sandbox.id
  ingress     = []
  egress      = []
}

resource "aws_security_group" "support" {
  name        = "${local.name_prefix}-support"
  description = "Private support host; HTTPS from gateway tasks and itself."
  vpc_id      = aws_vpc.sandbox.id
  ingress     = []
  egress      = []
}

resource "aws_security_group" "alb" {
  name        = "${local.name_prefix}-alb"
  description = "Internal ALB; HTTP is reachable only from the support proxy."
  vpc_id      = aws_vpc.sandbox.id
  ingress     = []
  egress      = []
}

resource "aws_security_group" "control_database" {
  name        = "${local.name_prefix}-control-db"
  description = "Private control-plane PostgreSQL; gateway tasks and support bootstrap only."
  vpc_id      = aws_vpc.sandbox.id
  ingress     = []
  egress      = []
}

resource "aws_security_group" "target_databases" {
  name        = "${local.name_prefix}-target-dbs"
  description = "Private PostgreSQL/MySQL targets; gateway tasks and Vault support host only."
  vpc_id      = aws_vpc.sandbox.id
  ingress     = []
  egress      = []
}

resource "aws_vpc_security_group_egress_rule" "managed_instances_all" {
  security_group_id = aws_security_group.managed_instances.id
  ip_protocol       = "-1"
  cidr_ipv4         = "0.0.0.0/0"
  description       = "ECS agent, image, logging, and control-plane egress via the private NAT route."
}

resource "aws_vpc_security_group_ingress_rule" "tasks_from_alb" {
  security_group_id            = aws_security_group.tasks.id
  referenced_security_group_id = aws_security_group.alb.id
  ip_protocol                  = "tcp"
  from_port                    = 8080
  to_port                      = 8080
  description                  = "Gateway traffic from the internal ALB."
}

resource "aws_vpc_security_group_egress_rule" "tasks_to_support" {
  security_group_id            = aws_security_group.tasks.id
  referenced_security_group_id = aws_security_group.support.id
  ip_protocol                  = "tcp"
  from_port                    = 443
  to_port                      = 443
  description                  = "HTTPS OIDC and Vault endpoints."
}

resource "aws_vpc_security_group_egress_rule" "tasks_to_databases" {
  for_each = {
    postgres = 5432
    mysql    = 3306
  }

  security_group_id            = aws_security_group.tasks.id
  referenced_security_group_id = aws_security_group.target_databases.id
  ip_protocol                  = "tcp"
  from_port                    = each.value
  to_port                      = each.value
  description                  = "${title(each.key)} target and control database traffic."
}

resource "aws_vpc_security_group_egress_rule" "tasks_to_control_database" {
  security_group_id            = aws_security_group.tasks.id
  referenced_security_group_id = aws_security_group.control_database.id
  ip_protocol                  = "tcp"
  from_port                    = 5432
  to_port                      = 5432
  description                  = "Dedicated control-plane PostgreSQL connection."
}

resource "aws_vpc_security_group_ingress_rule" "support_from_tasks" {
  security_group_id            = aws_security_group.support.id
  referenced_security_group_id = aws_security_group.tasks.id
  ip_protocol                  = "tcp"
  from_port                    = 443
  to_port                      = 443
  description                  = "Gateway tasks to HTTPS OIDC and Vault endpoints."
}

resource "aws_vpc_security_group_ingress_rule" "support_from_self" {
  security_group_id            = aws_security_group.support.id
  referenced_security_group_id = aws_security_group.support.id
  ip_protocol                  = "tcp"
  from_port                    = 443
  to_port                      = 443
  description                  = "Local in-VPC smoke tests through the HTTPS proxy."
}

resource "aws_vpc_security_group_egress_rule" "support_https" {
  security_group_id = aws_security_group.support.id
  ip_protocol       = "tcp"
  from_port         = 443
  to_port           = 443
  cidr_ipv4         = "0.0.0.0/0"
  description       = "SSM, package repositories, and pinned container registries via NAT."
}

resource "aws_vpc_security_group_egress_rule" "support_http" {
  security_group_id = aws_security_group.support.id
  ip_protocol       = "tcp"
  from_port         = 80
  to_port           = 80
  cidr_ipv4         = "0.0.0.0/0"
  description       = "Package repositories via NAT."
}

resource "aws_vpc_security_group_egress_rule" "support_to_alb" {
  security_group_id            = aws_security_group.support.id
  referenced_security_group_id = aws_security_group.alb.id
  ip_protocol                  = "tcp"
  from_port                    = 80
  to_port                      = 80
  description                  = "Same-origin proxy to the internal gateway ALB."
}

resource "aws_vpc_security_group_egress_rule" "support_to_control_database" {
  security_group_id            = aws_security_group.support.id
  referenced_security_group_id = aws_security_group.control_database.id
  ip_protocol                  = "tcp"
  from_port                    = 5432
  to_port                      = 5432
  description                  = "One-time control database user bootstrap."
}

resource "aws_vpc_security_group_egress_rule" "support_to_target_databases" {
  for_each = {
    postgres = 5432
    mysql    = 3306
  }

  security_group_id            = aws_security_group.support.id
  referenced_security_group_id = aws_security_group.target_databases.id
  ip_protocol                  = "tcp"
  from_port                    = each.value
  to_port                      = each.value
  description                  = "Vault database plugins issue and revoke target database leases."
}

resource "aws_vpc_security_group_ingress_rule" "alb_from_support" {
  security_group_id            = aws_security_group.alb.id
  referenced_security_group_id = aws_security_group.support.id
  ip_protocol                  = "tcp"
  from_port                    = 80
  to_port                      = 80
  description                  = "Only the support host HTTPS proxy can reach the ALB listener."
}

resource "aws_vpc_security_group_egress_rule" "alb_to_tasks" {
  security_group_id            = aws_security_group.alb.id
  referenced_security_group_id = aws_security_group.tasks.id
  ip_protocol                  = "tcp"
  from_port                    = 8080
  to_port                      = 8080
  description                  = "Forward gateway requests to ECS tasks."
}

resource "aws_vpc_security_group_ingress_rule" "target_databases_from_tasks" {
  for_each = {
    postgres = 5432
    mysql    = 3306
  }

  security_group_id            = aws_security_group.target_databases.id
  referenced_security_group_id = aws_security_group.tasks.id
  ip_protocol                  = "tcp"
  from_port                    = each.value
  to_port                      = each.value
  description                  = "Gateway connection to registered target databases."
}

resource "aws_vpc_security_group_ingress_rule" "control_database_from_tasks" {
  security_group_id            = aws_security_group.control_database.id
  referenced_security_group_id = aws_security_group.tasks.id
  ip_protocol                  = "tcp"
  from_port                    = 5432
  to_port                      = 5432
  description                  = "Gateway access to its dedicated control-plane database."
}

resource "aws_vpc_security_group_ingress_rule" "control_database_from_support" {
  security_group_id            = aws_security_group.control_database.id
  referenced_security_group_id = aws_security_group.support.id
  ip_protocol                  = "tcp"
  from_port                    = 5432
  to_port                      = 5432
  description                  = "Control-plane bootstrap only; no support access to target databases."
}

resource "aws_vpc_security_group_ingress_rule" "target_databases_from_support" {
  for_each = {
    postgres = 5432
    mysql    = 3306
  }

  security_group_id            = aws_security_group.target_databases.id
  referenced_security_group_id = aws_security_group.support.id
  ip_protocol                  = "tcp"
  from_port                    = each.value
  to_port                      = each.value
  description                  = "Vault database plugins only; no operator or public database ingress."
}

resource "aws_db_subnet_group" "sandbox" {
  name       = "${local.name_prefix}-db"
  subnet_ids = aws_subnet.private[*].id

  tags = {
    Name = "${local.name_prefix}-db"
  }
}

resource "aws_db_parameter_group" "postgres" {
  name        = "${local.name_prefix}-postgres17"
  family      = "postgres17"
  description = "Require TLS for sandbox PostgreSQL connections."

  parameter {
    name         = "rds.force_ssl"
    value        = "1"
    apply_method = "pending-reboot"
  }
}

resource "aws_db_parameter_group" "mysql" {
  name        = "${local.name_prefix}-mysql84"
  family      = "mysql8.4"
  description = "Require TLS for sandbox MySQL connections."

  parameter {
    name         = "require_secure_transport"
    value        = "ON"
    apply_method = "pending-reboot"
  }
}

resource "aws_db_instance" "control" {
  identifier                  = "${local.name_prefix}-control-pg"
  engine                      = "postgres"
  engine_version              = "17.6"
  instance_class              = "db.t4g.small"
  db_name                     = "sqlgw_control"
  username                    = "sqlgw_master"
  manage_master_user_password = true
  allocated_storage           = 20
  storage_type                = "gp3"
  storage_encrypted           = true
  db_subnet_group_name        = aws_db_subnet_group.sandbox.name
  vpc_security_group_ids      = [aws_security_group.control_database.id]
  parameter_group_name        = aws_db_parameter_group.postgres.name
  publicly_accessible         = false
  multi_az                    = false
  backup_retention_period     = 0
  delete_automated_backups    = true
  deletion_protection         = false
  skip_final_snapshot         = true
  apply_immediately           = true
  auto_minor_version_upgrade  = false

  tags = {
    Name = "${local.name_prefix}-control-pg"
    Role = "control-plane"
  }
}

resource "aws_db_instance" "target_postgres" {
  identifier                  = "${local.name_prefix}-target-pg"
  engine                      = "postgres"
  engine_version              = "17.6"
  instance_class              = "db.t4g.small"
  db_name                     = "sqlgw_target_pg"
  username                    = "sqlgw_master"
  manage_master_user_password = true
  allocated_storage           = 20
  storage_type                = "gp3"
  storage_encrypted           = true
  db_subnet_group_name        = aws_db_subnet_group.sandbox.name
  vpc_security_group_ids      = [aws_security_group.target_databases.id]
  parameter_group_name        = aws_db_parameter_group.postgres.name
  publicly_accessible         = false
  multi_az                    = false
  backup_retention_period     = 0
  delete_automated_backups    = true
  deletion_protection         = false
  skip_final_snapshot         = true
  apply_immediately           = true
  auto_minor_version_upgrade  = false

  tags = {
    Name = "${local.name_prefix}-target-pg"
    Role = "postgres-target"
  }
}

resource "aws_db_instance" "target_mysql" {
  identifier                  = "${local.name_prefix}-target-mysql"
  engine                      = "mysql"
  engine_version              = "8.4.7"
  instance_class              = "db.t4g.small"
  db_name                     = "sqlgw_target_mysql"
  username                    = "sqlgw_master"
  manage_master_user_password = true
  allocated_storage           = 20
  storage_type                = "gp3"
  storage_encrypted           = true
  db_subnet_group_name        = aws_db_subnet_group.sandbox.name
  vpc_security_group_ids      = [aws_security_group.target_databases.id]
  parameter_group_name        = aws_db_parameter_group.mysql.name
  publicly_accessible         = false
  multi_az                    = false
  backup_retention_period     = 0
  delete_automated_backups    = true
  deletion_protection         = false
  skip_final_snapshot         = true
  apply_immediately           = true
  auto_minor_version_upgrade  = false

  tags = {
    Name = "${local.name_prefix}-target-mysql"
    Role = "mysql-target"
  }
}

resource "aws_secretsmanager_secret" "control_database_url" {
  name                    = "${local.name_prefix}/gateway/control-database-url"
  description             = "Populated by the temporary sandbox support-host bootstrap."
  recovery_window_in_days = 0
}

resource "aws_secretsmanager_secret" "vault_admin_token" {
  name                    = "${local.name_prefix}/gateway/vault-admin-token"
  description             = "Populated by the temporary sandbox support-host bootstrap."
  recovery_window_in_days = 0
}

resource "aws_secretsmanager_secret" "vault_runtime_token" {
  name                    = "${local.name_prefix}/gateway/vault-runtime-token"
  description             = "Populated by the temporary sandbox support-host bootstrap."
  recovery_window_in_days = 0
}

resource "aws_ssm_parameter" "test_ca" {
  name        = local.ca_parameter_name
  description = "Public test CA certificate generated by the temporary support-host bootstrap."
  type        = "String"
  value       = "pending-bootstrap"

  lifecycle {
    ignore_changes = [value]
  }
}

resource "aws_ecr_repository" "gateway" {
  name                 = "${local.name_prefix}/gateway"
  image_tag_mutability = "IMMUTABLE"
  force_delete         = true

  encryption_configuration {
    encryption_type = "AES256"
  }
}

resource "aws_ecr_repository" "admin" {
  name                 = "${local.name_prefix}/admin"
  image_tag_mutability = "IMMUTABLE"
  force_delete         = true

  encryption_configuration {
    encryption_type = "AES256"
  }
}

resource "aws_s3_bucket" "bootstrap" {
  bucket        = "${local.name_prefix}-${data.aws_caller_identity.current.account_id}-bootstrap"
  force_destroy = true

  tags = {
    Name = "${local.name_prefix}-bootstrap"
  }
}

resource "aws_s3_bucket_public_access_block" "bootstrap" {
  bucket                  = aws_s3_bucket.bootstrap.id
  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

resource "aws_s3_bucket_ownership_controls" "bootstrap" {
  bucket = aws_s3_bucket.bootstrap.id

  rule {
    object_ownership = "BucketOwnerEnforced"
  }
}

resource "aws_s3_bucket_server_side_encryption_configuration" "bootstrap" {
  bucket = aws_s3_bucket.bootstrap.id

  rule {
    apply_server_side_encryption_by_default {
      sse_algorithm = "AES256"
    }
  }
}

resource "aws_s3_object" "support_bootstrap" {
  bucket = aws_s3_bucket.bootstrap.id
  key    = "${local.bootstrap_key_prefix}/bootstrap.sh"
  content = templatefile("${path.module}/support-host-bootstrap.sh.tftpl", {
    bootstrap_bucket     = aws_s3_bucket.bootstrap.bucket
    bootstrap_key_prefix = local.bootstrap_key_prefix
    sandbox_suffix       = var.sandbox_suffix
    domain               = local.domain
  })
  content_type = "text/x-shellscript"

  depends_on = [
    aws_s3_bucket_public_access_block.bootstrap,
    aws_s3_bucket_ownership_controls.bootstrap,
    aws_s3_bucket_server_side_encryption_configuration.bootstrap,
  ]
}

resource "aws_s3_object" "support_config" {
  bucket       = aws_s3_bucket.bootstrap.id
  key          = "${local.bootstrap_key_prefix}/config.json"
  content      = jsonencode(local.bootstrap_config)
  content_type = "application/json"

  depends_on = [
    aws_s3_bucket_public_access_block.bootstrap,
    aws_s3_bucket_ownership_controls.bootstrap,
    aws_s3_bucket_server_side_encryption_configuration.bootstrap,
  ]
}

resource "aws_s3_object" "compose" {
  bucket       = aws_s3_bucket.bootstrap.id
  key          = "${local.bootstrap_key_prefix}/compose.yaml"
  source       = "${path.module}/compose.ecs-mi-sandbox.yaml"
  source_hash  = filemd5("${path.module}/compose.ecs-mi-sandbox.yaml")
  content_type = "application/yaml"

  depends_on = [
    aws_s3_bucket_public_access_block.bootstrap,
    aws_s3_bucket_ownership_controls.bootstrap,
    aws_s3_bucket_server_side_encryption_configuration.bootstrap,
  ]
}

resource "aws_s3_object" "caddyfile" {
  bucket = aws_s3_bucket.bootstrap.id
  key    = "${local.bootstrap_key_prefix}/Caddyfile"
  content = templatefile("${path.module}/Caddyfile.tftpl", {
    domain       = local.domain
    alb_dns_name = aws_lb.gateway.dns_name
  })
  content_type = "text/plain"

  depends_on = [
    aws_s3_bucket_public_access_block.bootstrap,
    aws_s3_bucket_ownership_controls.bootstrap,
    aws_s3_bucket_server_side_encryption_configuration.bootstrap,
  ]
}

resource "aws_s3_object" "vault_bootstrap" {
  bucket       = aws_s3_bucket.bootstrap.id
  key          = "${local.bootstrap_key_prefix}/dev-vault-bootstrap.sh"
  source       = "${path.module}/../../../scripts/dev-vault-bootstrap.sh"
  source_hash  = filemd5("${path.module}/../../../scripts/dev-vault-bootstrap.sh")
  content_type = "text/x-shellscript"

  depends_on = [
    aws_s3_bucket_public_access_block.bootstrap,
    aws_s3_bucket_ownership_controls.bootstrap,
    aws_s3_bucket_server_side_encryption_configuration.bootstrap,
  ]
}

resource "aws_s3_object" "authentik_blueprint" {
  bucket       = aws_s3_bucket.bootstrap.id
  key          = "${local.bootstrap_key_prefix}/authentik-blueprint.yaml"
  source       = "${path.module}/../../../deploy/authentik/sql-mcp-gateway-dev.yaml"
  source_hash  = filemd5("${path.module}/../../../deploy/authentik/sql-mcp-gateway-dev.yaml")
  content_type = "application/yaml"

  depends_on = [
    aws_s3_bucket_public_access_block.bootstrap,
    aws_s3_bucket_ownership_controls.bootstrap,
    aws_s3_bucket_server_side_encryption_configuration.bootstrap,
  ]
}

resource "aws_lb" "gateway" {
  name                       = "${local.name_prefix}-gateway"
  internal                   = true
  load_balancer_type         = "application"
  subnets                    = aws_subnet.private[*].id
  security_groups            = [aws_security_group.alb.id]
  enable_deletion_protection = false

  tags = {
    Name = "${local.name_prefix}-gateway"
  }
}

resource "aws_lb_target_group" "gateway" {
  name        = "${local.name_prefix}-gateway"
  port        = 8080
  protocol    = "HTTP"
  target_type = "ip"
  vpc_id      = aws_vpc.sandbox.id

  health_check {
    enabled             = true
    path                = "/readyz"
    protocol            = "HTTP"
    matcher             = "204"
    healthy_threshold   = 2
    unhealthy_threshold = 3
    interval            = 15
    timeout             = 5
  }
}

resource "aws_lb_listener" "gateway" {
  load_balancer_arn = aws_lb.gateway.arn
  port              = 80
  protocol          = "HTTP"

  default_action {
    type             = "forward"
    target_group_arn = aws_lb_target_group.gateway.arn
  }
}

resource "aws_route53_zone" "sandbox" {
  name = local.domain

  vpc {
    vpc_id = aws_vpc.sandbox.id
  }

  comment = "Private DNS for temporary SQL MCP Gateway sandbox ${var.sandbox_suffix}."
}

resource "aws_iam_role" "support" {
  name = "${local.name_prefix}-support"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Service = "ec2.amazonaws.com" }
      Action    = "sts:AssumeRole"
    }]
  })
}

resource "aws_iam_role_policy_attachment" "support_ssm" {
  role       = aws_iam_role.support.name
  policy_arn = "arn:${data.aws_partition.current.partition}:iam::aws:policy/AmazonSSMManagedInstanceCore"
}

resource "aws_iam_role_policy" "support_bootstrap" {
  name = "${local.name_prefix}-bootstrap"
  role = aws_iam_role.support.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect   = "Allow"
        Action   = ["secretsmanager:GetSecretValue"]
        Resource = local.database_secret_arns
      },
      {
        Effect   = "Allow"
        Action   = ["secretsmanager:PutSecretValue"]
        Resource = local.gateway_secret_arns
      },
      {
        Effect   = "Allow"
        Action   = ["ssm:PutParameter"]
        Resource = aws_ssm_parameter.test_ca.arn
      },
      {
        Effect   = "Allow"
        Action   = ["ecr:GetAuthorizationToken"]
        Resource = "*"
      },
      {
        Effect = "Allow"
        Action = [
          "ecr:BatchCheckLayerAvailability",
          "ecr:BatchGetImage",
          "ecr:GetDownloadUrlForLayer",
        ]
        Resource = [aws_ecr_repository.admin.arn]
      },
      {
        Effect = "Allow"
        Action = ["s3:GetObject"]
        Resource = [
          aws_s3_object.support_bootstrap.arn,
          aws_s3_object.support_config.arn,
          aws_s3_object.compose.arn,
          aws_s3_object.caddyfile.arn,
          aws_s3_object.vault_bootstrap.arn,
          aws_s3_object.authentik_blueprint.arn,
        ]
      },
    ]
  })
}

resource "aws_iam_instance_profile" "support" {
  name = "${local.name_prefix}-support"
  role = aws_iam_role.support.name
}

resource "aws_instance" "support" {
  ami                         = data.aws_ami.amazon_linux_2023.id
  instance_type               = "t3.large"
  subnet_id                   = aws_subnet.private[0].id
  vpc_security_group_ids      = [aws_security_group.support.id]
  iam_instance_profile        = aws_iam_instance_profile.support.name
  associate_public_ip_address = false
  user_data = templatefile("${path.module}/support-host-user-data.sh.tftpl", {
    aws_region           = var.aws_region
    bootstrap_bucket     = aws_s3_bucket.bootstrap.bucket
    bootstrap_key_prefix = local.bootstrap_key_prefix
  })
  user_data_replace_on_change = true

  metadata_options {
    http_endpoint               = "enabled"
    http_tokens                 = "required"
    http_put_response_hop_limit = 1
  }

  root_block_device {
    encrypted             = true
    volume_size           = 20
    volume_type           = "gp3"
    delete_on_termination = true
  }

  depends_on = [
    aws_db_instance.control,
    aws_db_instance.target_postgres,
    aws_db_instance.target_mysql,
    aws_iam_role_policy.support_bootstrap,
    aws_iam_role_policy_attachment.support_ssm,
    aws_route_table_association.private,
    aws_s3_object.support_bootstrap,
    aws_s3_object.support_config,
    aws_s3_object.compose,
    aws_s3_object.caddyfile,
    aws_s3_object.vault_bootstrap,
    aws_s3_object.authentik_blueprint,
  ]

  tags = {
    Name = "${local.name_prefix}-support"
    Role = "support"
  }
}

resource "aws_route53_record" "auth" {
  zone_id = aws_route53_zone.sandbox.zone_id
  name    = "auth.${local.domain}"
  type    = "A"
  ttl     = 30
  records = [aws_instance.support.private_ip]
}

resource "aws_route53_record" "vault" {
  zone_id = aws_route53_zone.sandbox.zone_id
  name    = "vault.${local.domain}"
  type    = "A"
  ttl     = 30
  records = [aws_instance.support.private_ip]
}

resource "aws_route53_record" "gateway" {
  zone_id = aws_route53_zone.sandbox.zone_id
  name    = "gateway.${local.domain}"
  type    = "A"
  ttl     = 30
  records = [aws_instance.support.private_ip]
}
