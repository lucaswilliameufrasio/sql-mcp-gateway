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
  region = var.aws_region

  default_tags {
    tags = {
      Application = "sql-mcp-gateway"
      Environment = var.environment
      ManagedBy   = "OpenTofu"
    }
  }
}

data "aws_partition" "current" {}

locals {
  required_secret_arns = [
    var.control_database_url_secret_arn,
    var.vault_admin_token_secret_arn,
    var.vault_runtime_token_secret_arn,
  ]
}

resource "aws_ecs_cluster" "gateway" {
  name = var.name

  setting {
    name  = "containerInsights"
    value = "enabled"
  }
}

resource "aws_iam_role" "infrastructure" {
  name = "${var.name}-mi-infrastructure"

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
  name = "ecsInstanceRole-${var.name}"

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
  name = "${var.name}-mi-instance-profile"
  role = aws_iam_role.instance.name
}

resource "aws_iam_role" "task_execution" {
  name = "${var.name}-task-execution"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Principal = {
        Service = "ecs-tasks.amazonaws.com"
      }
      Action = "sts:AssumeRole"
    }]
  })
}

resource "aws_iam_role_policy_attachment" "task_execution" {
  role       = aws_iam_role.task_execution.name
  policy_arn = "arn:${data.aws_partition.current.partition}:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy"
}

resource "aws_iam_role_policy" "read_secrets" {
  name = "${var.name}-read-task-secrets"
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
  count = length(var.secret_kms_key_arns) > 0 ? 1 : 0

  name = "${var.name}-decrypt-task-secrets"
  role = aws_iam_role.task_execution.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect   = "Allow"
      Action   = ["kms:Decrypt"]
      Resource = var.secret_kms_key_arns
    }]
  })
}

resource "aws_iam_role" "task" {
  name = "${var.name}-task"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Principal = {
        Service = "ecs-tasks.amazonaws.com"
      }
      Action = "sts:AssumeRole"
    }]
  })
}

resource "aws_cloudwatch_log_group" "gateway" {
  name              = "/ecs/${var.name}/gateway"
  retention_in_days = var.log_retention_in_days
}

resource "aws_ecs_capacity_provider" "managed_instances" {
  name    = "${var.name}-managed-instances"
  cluster = aws_ecs_cluster.gateway.name

  managed_instances_provider {
    infrastructure_role_arn = aws_iam_role.infrastructure.arn
    propagate_tags          = "CAPACITY_PROVIDER"

    instance_launch_template {
      ec2_instance_profile_arn = aws_iam_instance_profile.instance.arn
      capacity_option_type     = "ON_DEMAND"
      monitoring               = "DETAILED"

      network_configuration {
        subnets         = var.private_subnet_ids
        security_groups = var.managed_instance_security_group_ids
      }

      storage_configuration {
        storage_size_gib = var.instance_storage_size_gib
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

        instance_generations = ["current"]
      }
    }
  }

  depends_on = [
    aws_iam_role_policy_attachment.infrastructure,
    aws_iam_role_policy_attachment.instance,
  ]
}

resource "aws_ecs_cluster_capacity_providers" "gateway" {
  cluster_name       = aws_ecs_cluster.gateway.name
  capacity_providers = [aws_ecs_capacity_provider.managed_instances.name]

  default_capacity_provider_strategy {
    capacity_provider = aws_ecs_capacity_provider.managed_instances.name
    weight            = 1
  }
}

resource "aws_ecs_task_definition" "gateway" {
  family                   = var.name
  network_mode             = "awsvpc"
  requires_compatibilities = ["MANAGED_INSTANCES"]
  cpu                      = tostring(var.task_cpu)
  memory                   = tostring(var.task_memory_mib)
  execution_role_arn       = aws_iam_role.task_execution.arn
  task_role_arn            = aws_iam_role.task.arn

  runtime_platform {
    operating_system_family = "LINUX"
    cpu_architecture        = var.cpu_architecture
  }

  container_definitions = jsonencode([{
    name        = "sql-mcp-gateway"
    image       = var.image_uri
    essential   = true
    user        = "10001"
    stopTimeout = 60

    portMappings = [{
      containerPort = 8080
      protocol      = "tcp"
    }]

    environment = [
      { name = "BIND_ADDRESS", value = "0.0.0.0:8080" },
      { name = "OIDC_ISSUER_URL", value = var.oidc_issuer_url },
      { name = "OIDC_CLIENT_ID", value = var.oidc_client_id },
      { name = "OIDC_ACCESS_TOKEN_AUDIENCE", value = var.oidc_access_token_audience },
      { name = "OIDC_BOOTSTRAP_ADMIN_SUBJECTS", value = join(",", var.oidc_bootstrap_admin_subjects) },
      { name = "VAULT_ADDRESS", value = var.vault_address },
      { name = "MAX_QUERY_ROWS", value = tostring(var.max_query_rows) },
      { name = "MAX_QUERY_DURATION_SECONDS", value = tostring(var.max_query_duration_seconds) },
    ]

    secrets = [
      { name = "CONTROL_DATABASE_URL", valueFrom = var.control_database_url_secret_arn },
      { name = "VAULT_ADMIN_TOKEN", valueFrom = var.vault_admin_token_secret_arn },
      { name = "VAULT_RUNTIME_TOKEN", valueFrom = var.vault_runtime_token_secret_arn },
    ]

    logConfiguration = {
      logDriver = "awslogs"
      options = {
        awslogs-group         = aws_cloudwatch_log_group.gateway.name
        awslogs-region        = var.aws_region
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
  name                               = "${var.name}-gateway"
  cluster                            = aws_ecs_cluster.gateway.id
  task_definition                    = aws_ecs_task_definition.gateway.arn
  desired_count                      = var.desired_count
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
    subnets          = var.private_subnet_ids
    security_groups  = var.task_security_group_ids
    assign_public_ip = false
  }

  load_balancer {
    target_group_arn = var.private_alb_target_group_arn
    container_name   = "sql-mcp-gateway"
    container_port   = 8080
  }

  ordered_placement_strategy {
    type  = "spread"
    field = "attribute:ecs.availability-zone"
  }

  depends_on = [aws_ecs_cluster_capacity_providers.gateway]
}
