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
    tags = {
      Application = "sql-mcp-gateway"
      Environment = local.environment
      ManagedBy   = "OpenTofu"
    }
  }
}

data "aws_partition" "current" {}

resource "aws_cloudwatch_log_group" "lambda" {
  name              = "/aws/lambda/${local.name}"
  retention_in_days = local.log_retention_in_days
}

resource "aws_cloudwatch_log_group" "api_access" {
  name              = "/aws/apigateway/${local.name}/access"
  retention_in_days = local.log_retention_in_days
}

resource "aws_iam_role" "lambda" {
  name = "${local.name}-execution"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Principal = {
        Service = "lambda.amazonaws.com"
      }
      Action = "sts:AssumeRole"
    }]
  })
}

resource "aws_iam_role_policy_attachment" "lambda_vpc" {
  role       = aws_iam_role.lambda.name
  policy_arn = "arn:${data.aws_partition.current.partition}:iam::aws:policy/service-role/AWSLambdaVPCAccessExecutionRole"
}

resource "aws_iam_role_policy" "read_secrets" {
  name = "${local.name}-read-runtime-secrets"
  role = aws_iam_role.lambda.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Action = ["secretsmanager:GetSecretValue"]
      Resource = [
        local.control_database_url_secret_arn,
        local.vault_credentials_secret_arn,
      ]
    }]
  })
}

resource "aws_iam_role_policy" "decrypt_secrets" {
  count = length(local.secret_kms_key_arns) > 0 ? 1 : 0

  name = "${local.name}-decrypt-runtime-secrets"
  role = aws_iam_role.lambda.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect   = "Allow"
      Action   = ["kms:Decrypt"]
      Resource = local.secret_kms_key_arns
    }]
  })
}

resource "aws_lambda_function" "gateway" {
  function_name = local.name
  package_type  = "Image"
  image_uri     = local.lambda_image_uri
  role          = aws_iam_role.lambda.arn
  architectures = ["x86_64"]
  memory_size   = local.lambda_memory_mb
  timeout       = local.lambda_timeout_seconds

  reserved_concurrent_executions = local.reserved_concurrent_executions

  ephemeral_storage {
    size = 512
  }

  vpc_config {
    subnet_ids         = local.private_subnet_ids
    security_group_ids = local.lambda_security_group_ids
  }

  environment {
    variables = {
      BIND_ADDRESS                            = "0.0.0.0:8080"
      PORT                                    = "8080"
      AWS_LWA_PORT                            = "8080"
      AWS_LWA_READINESS_CHECK_PATH            = "/health"
      AWS_LWA_READINESS_CHECK_TIMEOUT_SECONDS = "20"
      AWS_LWA_ASYNC_INIT                      = "true"
      CONTROL_DATABASE_URL_SECRET_ARN         = local.control_database_url_secret_arn
      VAULT_CREDENTIALS_SECRET_ARN            = local.vault_credentials_secret_arn
      OIDC_ISSUER_URL                         = local.oidc_issuer_url
      OIDC_CLIENT_ID                          = local.oidc_client_id
      OIDC_ACCESS_TOKEN_AUDIENCE              = local.oidc_access_token_audience
      OIDC_BOOTSTRAP_ADMIN_SUBJECTS           = join(",", local.oidc_bootstrap_admin_subjects)
      VAULT_ADDRESS                           = local.vault_address
      MAX_QUERY_ROWS                          = tostring(local.max_query_rows)
      MAX_QUERY_DURATION_SECONDS              = tostring(local.max_query_duration_seconds)
    }
  }

  lifecycle {
    precondition {
      condition     = can(regex("^[A-Za-z0-9_-]{3,32}$", local.name))
      error_message = "name must be 3-32 letters, digits, hyphens, or underscores."
    }

    precondition {
      condition     = length(local.private_subnet_ids) >= 2 && length(local.lambda_security_group_ids) > 0
      error_message = "Provide at least two private subnets and one Lambda security group."
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
      condition     = local.lambda_memory_mb >= 128 && local.lambda_memory_mb <= 10240
      error_message = "lambda_memory_mb must be between 128 and 10240."
    }

    precondition {
      condition     = local.lambda_timeout_seconds >= 1 && local.lambda_timeout_seconds <= 29
      error_message = "lambda_timeout_seconds must be between 1 and 29 for this API Gateway example."
    }

    precondition {
      condition     = local.reserved_concurrent_executions >= 1
      error_message = "reserved_concurrent_executions must be at least 1."
    }
  }

  depends_on = [
    aws_cloudwatch_log_group.lambda,
    aws_iam_role_policy.read_secrets,
    aws_iam_role_policy.decrypt_secrets,
    aws_iam_role_policy_attachment.lambda_vpc,
  ]
}

# The VPC access policy is needed by the Lambda service to manage Hyperplane
# ENIs. This condition denies the gateway process itself those EC2 API actions.
resource "aws_iam_role_policy" "deny_runtime_eni_management" {
  name = "${local.name}-deny-runtime-eni-management"
  role = aws_iam_role.lambda.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Deny"
      Action = [
        "ec2:AssignPrivateIpAddresses",
        "ec2:CreateNetworkInterface",
        "ec2:DeleteNetworkInterface",
        "ec2:DescribeNetworkInterfaces",
        "ec2:DescribeSubnets",
        "ec2:DetachNetworkInterface",
        "ec2:UnassignPrivateIpAddresses",
      ]
      Resource = "*"
      Condition = {
        ArnEquals = {
          "lambda:SourceFunctionArn" = aws_lambda_function.gateway.arn
        }
      }
    }]
  })
}

resource "aws_lambda_permission" "api_gateway" {
  statement_id  = "AllowPrivateApiGatewayInvoke"
  action        = "lambda:InvokeFunction"
  function_name = aws_lambda_function.gateway.function_name
  principal     = "apigateway.amazonaws.com"
  source_arn    = "${aws_api_gateway_rest_api.private.execution_arn}/*/*/*"
}

resource "aws_api_gateway_rest_api" "private" {
  name = "${local.name}-private"

  endpoint_configuration {
    types            = ["PRIVATE"]
    vpc_endpoint_ids = [local.execute_api_vpc_endpoint_id]
  }

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = "*"
      Action    = "execute-api:Invoke"
      Resource  = "execute-api:/*"
      Condition = {
        StringEquals = {
          "aws:SourceVpce" = local.execute_api_vpc_endpoint_id
        }
      }
    }]
  })
}

resource "aws_api_gateway_resource" "proxy" {
  rest_api_id = aws_api_gateway_rest_api.private.id
  parent_id   = aws_api_gateway_rest_api.private.root_resource_id
  path_part   = "{proxy+}"
}

resource "aws_api_gateway_method" "root" {
  rest_api_id   = aws_api_gateway_rest_api.private.id
  resource_id   = aws_api_gateway_rest_api.private.root_resource_id
  http_method   = "ANY"
  authorization = "NONE"
}

resource "aws_api_gateway_method" "proxy" {
  rest_api_id   = aws_api_gateway_rest_api.private.id
  resource_id   = aws_api_gateway_resource.proxy.id
  http_method   = "ANY"
  authorization = "NONE"

  request_parameters = {
    "method.request.path.proxy" = true
  }
}

resource "aws_api_gateway_integration" "root" {
  rest_api_id             = aws_api_gateway_rest_api.private.id
  resource_id             = aws_api_gateway_rest_api.private.root_resource_id
  http_method             = aws_api_gateway_method.root.http_method
  integration_http_method = "POST"
  type                    = "AWS_PROXY"
  uri                     = "arn:${data.aws_partition.current.partition}:apigateway:${local.aws_region}:lambda:path/2015-03-31/functions/${aws_lambda_function.gateway.invoke_arn}/invocations"
  timeout_milliseconds    = 29000
}

resource "aws_api_gateway_integration" "proxy" {
  rest_api_id             = aws_api_gateway_rest_api.private.id
  resource_id             = aws_api_gateway_resource.proxy.id
  http_method             = aws_api_gateway_method.proxy.http_method
  integration_http_method = "POST"
  type                    = "AWS_PROXY"
  uri                     = "arn:${data.aws_partition.current.partition}:apigateway:${local.aws_region}:lambda:path/2015-03-31/functions/${aws_lambda_function.gateway.invoke_arn}/invocations"
  timeout_milliseconds    = 29000
}

resource "aws_api_gateway_deployment" "private" {
  rest_api_id = aws_api_gateway_rest_api.private.id

  triggers = {
    redeployment = sha1(jsonencode({
      methods = {
        root_http_method         = aws_api_gateway_method.root.http_method
        root_authorization       = aws_api_gateway_method.root.authorization
        proxy_http_method        = aws_api_gateway_method.proxy.http_method
        proxy_authorization      = aws_api_gateway_method.proxy.authorization
        proxy_request_parameters = aws_api_gateway_method.proxy.request_parameters
      }
      integrations = {
        root_uri      = aws_api_gateway_integration.root.uri
        root_type     = aws_api_gateway_integration.root.type
        root_timeout  = aws_api_gateway_integration.root.timeout_milliseconds
        proxy_uri     = aws_api_gateway_integration.proxy.uri
        proxy_type    = aws_api_gateway_integration.proxy.type
        proxy_timeout = aws_api_gateway_integration.proxy.timeout_milliseconds
      }
      resource_policy = aws_api_gateway_rest_api.private.policy
    }))
  }

  depends_on = [
    aws_api_gateway_integration.root,
    aws_api_gateway_integration.proxy,
  ]

  lifecycle {
    create_before_destroy = true
  }
}

resource "aws_api_gateway_stage" "private" {
  rest_api_id   = aws_api_gateway_rest_api.private.id
  deployment_id = aws_api_gateway_deployment.private.id
  stage_name    = local.api_stage_name

  access_log_settings {
    destination_arn = aws_cloudwatch_log_group.api_access.arn
    format = jsonencode({
      requestId      = "$context.requestId"
      sourceIp       = "$context.identity.sourceIp"
      requestTime    = "$context.requestTime"
      httpMethod     = "$context.httpMethod"
      resourcePath   = "$context.resourcePath"
      status         = "$context.status"
      responseLength = "$context.responseLength"
    })
  }

  depends_on = [
    aws_iam_role_policy.deny_runtime_eni_management,
    aws_lambda_permission.api_gateway,
  ]
}

resource "aws_api_gateway_method_settings" "all" {
  rest_api_id = aws_api_gateway_rest_api.private.id
  stage_name  = aws_api_gateway_stage.private.stage_name
  method_path = "*/*"

  settings {
    metrics_enabled        = true
    logging_level          = "INFO"
    data_trace_enabled     = false
    throttling_rate_limit  = local.api_rate_limit_per_second
    throttling_burst_limit = local.api_burst_limit
  }
}
