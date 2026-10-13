output "private_api_invoke_url" {
  description = "Private API Gateway stage URL; reachable only through the configured execute-api VPC endpoint."
  value       = aws_api_gateway_stage.private.invoke_url
}

output "lambda_function_arn" {
  description = "Lambda function ARN."
  value       = aws_lambda_function.gateway.arn
}

output "lambda_execution_role_arn" {
  description = "Lambda execution role ARN."
  value       = aws_iam_role.lambda.arn
}

output "lambda_log_group_name" {
  description = "CloudWatch log group for the Lambda runtime."
  value       = aws_cloudwatch_log_group.lambda.name
}
