terraform {
  required_version = ">= 1.8.0"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = ">= 6.30.0, < 7.0.0"
    }
  }
}

provider "aws" {
  region = local.aws_region

  default_tags {
    tags = merge(
      {
        Application = "sql-mcp-gateway"
        Environment = local.environment
        ManagedBy   = "OpenTofu"
      },
      local.sandbox_id == "" ? {} : { SandboxId = local.sandbox_id },
    )
  }
}

data "aws_partition" "current" {}
data "aws_caller_identity" "current" {}

locals {
  ecs_tasks_assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Principal = {
        Service = "ecs-tasks.amazonaws.com"
      }
      Action = "sts:AssumeRole"
      Condition = {
        ArnLike = {
          "aws:SourceArn" = "arn:${data.aws_partition.current.partition}:ecs:${local.aws_region}:${data.aws_caller_identity.current.account_id}:*"
        }
        StringEquals = {
          "aws:SourceAccount" = data.aws_caller_identity.current.account_id
        }
      }
    }]
  })

  required_secret_arns = [
    local.control_database_url_secret_arn,
    local.mcp_oauth_state_key_secret_arn,
    local.vault_admin_token_secret_arn,
    local.vault_runtime_token_secret_arn,
  ]
}

resource "aws_ecs_cluster" "gateway" {
  name = local.name

  setting {
    name  = "containerInsights"
    value = "enabled"
  }
}

resource "aws_iam_role" "infrastructure" {
  name = "${local.name}-mi-infrastructure"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Principal = {
        Service = "ecs.amazonaws.com"
      }
      Action = "sts:AssumeRole"
    }]
  })
}

resource "aws_iam_role_policy_attachment" "infrastructure" {
  role       = aws_iam_role.infrastructure.name
  policy_arn = "arn:${data.aws_partition.current.partition}:iam::aws:policy/AmazonECSInfrastructureRolePolicyForManagedInstances"
}

resource "aws_iam_role" "instance" {
  name = "ecsInstanceRole-${local.name}"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Principal = {
        Service = "ec2.amazonaws.com"
      }
      Action = "sts:AssumeRole"
    }]
  })
}

resource "aws_iam_role_policy_attachment" "instance" {
  role       = aws_iam_role.instance.name
  policy_arn = "arn:${data.aws_partition.current.partition}:iam::aws:policy/AmazonECSInstanceRolePolicyForManagedInstances"
}

resource "aws_iam_instance_profile" "instance" {
  name = "${local.name}-mi-instance-profile"
  role = aws_iam_role.instance.name
}

resource "aws_iam_role" "task_execution" {
  name = "${local.name}-task-execution"

  assume_role_policy = local.ecs_tasks_assume_role_policy
}

resource "aws_iam_role_policy_attachment" "task_execution" {
  role       = aws_iam_role.task_execution.name
  policy_arn = "arn:${data.aws_partition.current.partition}:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy"
}

resource "aws_iam_role_policy" "read_secrets" {
  name = "${local.name}-read-task-secrets"
  role = aws_iam_role.task_execution.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect   = "Allow"
      Action   = ["secretsmanager:GetSecretValue"]
      Resource = local.required_secret_arns
    }]
  })
}

resource "aws_iam_role_policy" "decrypt_secrets" {
  count = length(local.secret_kms_key_arns) > 0 ? 1 : 0

  name = "${local.name}-decrypt-task-secrets"
  role = aws_iam_role.task_execution.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect   = "Allow"
      Action   = ["kms:Decrypt"]
      Resource = local.secret_kms_key_arns
    }]
  })
}

resource "aws_iam_role" "task" {
  name = "${local.name}-task"

  assume_role_policy = local.ecs_tasks_assume_role_policy
}

resource "aws_cloudwatch_log_group" "gateway" {
  name              = "/ecs/${local.name}/gateway"
  retention_in_days = local.log_retention_in_days
}

resource "aws_ecs_capacity_provider" "managed_instances" {
  name    = "${local.name}-managed-instances"
  cluster = aws_ecs_cluster.gateway.name

  managed_instances_provider {
    infrastructure_role_arn = aws_iam_role.infrastructure.arn
    propagate_tags          = "CAPACITY_PROVIDER"

    instance_launch_template {
      ec2_instance_profile_arn = aws_iam_instance_profile.instance.arn
      capacity_option_type     = "ON_DEMAND"
      monitoring               = "DETAILED"

      network_configuration {
        subnets         = local.private_subnet_ids
        security_groups = local.managed_instance_security_group_ids
      }

      storage_configuration {
        storage_size_gib = local.instance_storage_size_gib
      }

      instance_requirements {
        vcpu_count {
          min = 2
          max = 16
        }

        memory_mib {
          min = 4096
          max = 32768
        }

        allowed_instance_types = length(local.allowed_instance_types) > 0 ? local.allowed_instance_types : null
      }
    }
  }

  depends_on = [
    aws_iam_role_policy_attachment.infrastructure,
    aws_iam_role_policy_attachment.instance,
  ]
}

resource "aws_ecs_task_definition" "gateway" {
  family                   = local.name
  network_mode             = "awsvpc"
  requires_compatibilities = ["MANAGED_INSTANCES"]
  cpu                      = tostring(local.task_cpu)
  memory                   = tostring(local.task_memory_mib)
  execution_role_arn       = aws_iam_role.task_execution.arn
  task_role_arn            = aws_iam_role.task.arn

  runtime_platform {
    operating_system_family = "LINUX"
    cpu_architecture        = local.cpu_architecture
  }

  container_definitions = jsonencode([{
    name        = "sql-mcp-gateway"
    image       = local.image_uri
    essential   = true
    user        = "10001"
    stopTimeout = 60

    portMappings = [{
      containerPort = 8080
      protocol      = "tcp"
    }]

    environment = [
      { name = "BIND_ADDRESS", value = "0.0.0.0:8080" },
      { name = "OIDC_ISSUER_URL", value = local.oidc_issuer_url },
      { name = "OIDC_CLIENT_ID", value = local.oidc_client_id },
      { name = "OIDC_ACCESS_TOKEN_AUDIENCE", value = local.oidc_access_token_audience },
      { name = "MCP_PUBLIC_URL", value = local.mcp_public_url },
      { name = "OIDC_BOOTSTRAP_ADMIN_SUBJECTS", value = join(",", local.oidc_bootstrap_admin_subjects) },
      { name = "VAULT_ADDRESS", value = local.vault_address },
      { name = "MAX_QUERY_ROWS", value = tostring(local.max_query_rows) },
      { name = "MAX_QUERY_DURATION_SECONDS", value = tostring(local.max_query_duration_seconds) },
    ]

    secrets = [
      { name = "CONTROL_DATABASE_URL", valueFrom = local.control_database_url_secret_arn },
      { name = "MCP_OAUTH_STATE_KEY", valueFrom = local.mcp_oauth_state_key_secret_arn },
      { name = "VAULT_ADMIN_TOKEN", valueFrom = local.vault_admin_token_secret_arn },
      { name = "VAULT_RUNTIME_TOKEN", valueFrom = local.vault_runtime_token_secret_arn },
    ]

    logConfiguration = {
      logDriver = "awslogs"
      options = {
        awslogs-group         = aws_cloudwatch_log_group.gateway.name
        awslogs-region        = local.aws_region
        awslogs-stream-prefix = "gateway"
      }
    }
  }])

  depends_on = [
    aws_iam_role_policy.read_secrets,
    aws_iam_role_policy.decrypt_secrets,
    aws_iam_role_policy_attachment.task_execution,
    aws_cloudwatch_log_group.gateway,
  ]
}

resource "aws_ecs_service" "gateway" {
  name                               = "${local.name}-gateway"
  cluster                            = aws_ecs_cluster.gateway.id
  task_definition                    = aws_ecs_task_definition.gateway.arn
  desired_count                      = local.desired_count
  propagate_tags                     = "SERVICE"
  health_check_grace_period_seconds  = 90
  deployment_minimum_healthy_percent = 50
  deployment_maximum_percent         = 200

  deployment_circuit_breaker {
    enable   = true
    rollback = true
  }

  capacity_provider_strategy {
    capacity_provider = aws_ecs_capacity_provider.managed_instances.name
    weight            = 1
  }

  network_configuration {
    subnets          = local.private_subnet_ids
    security_groups  = local.task_security_group_ids
    assign_public_ip = false
  }

  load_balancer {
    target_group_arn = local.private_alb_target_group_arn
    container_name   = "sql-mcp-gateway"
    container_port   = 8080
  }

  lifecycle {
    precondition {
      condition     = can(regex("^[A-Za-z0-9_-]{3,32}$", local.name))
      error_message = "name must be 3-32 letters, digits, hyphens, or underscores."
    }

    precondition {
      condition     = length(local.private_subnet_ids) >= 2
      error_message = "Provide private subnets in at least two Availability Zones."
    }

    precondition {
      condition = (
        length(local.managed_instance_security_group_ids) > 0 &&
        length(local.task_security_group_ids) > 0
      )
      error_message = "Provide managed-instance and task security groups."
    }

    precondition {
      condition     = length(local.oidc_bootstrap_admin_subjects) > 0
      error_message = "Provide at least one exact OIDC subject for initial administration."
    }

    precondition {
      condition     = local.max_query_rows >= 1 && local.max_query_rows <= 1000
      error_message = "max_query_rows must be between 1 and 1000."
    }

    precondition {
      condition     = local.max_query_duration_seconds >= 1 && local.max_query_duration_seconds <= 300
      error_message = "max_query_duration_seconds must be between 1 and 300."
    }

    precondition {
      condition     = contains(["X86_64", "ARM64"], local.cpu_architecture)
      error_message = "cpu_architecture must be X86_64 or ARM64 and match the published image."
    }

    precondition {
      condition     = local.desired_count >= 1
      error_message = "desired_count must be at least 1."
    }
  }

}
