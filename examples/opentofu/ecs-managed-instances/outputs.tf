output "ecs_cluster_name" {
  description = "ECS cluster hosting the gateway service."
  value       = aws_ecs_cluster.gateway.name
}

output "capacity_provider_name" {
  description = "ECS Managed Instances capacity provider associated with the cluster."
  value       = aws_ecs_capacity_provider.managed_instances.name
}

output "gateway_service_name" {
  description = "ECS service name."
  value       = aws_ecs_service.gateway.name
}

output "task_execution_role_arn" {
  description = "Execution role used by ECS to pull the image, write logs, and fetch referenced secrets."
  value       = aws_iam_role.task_execution.arn
}
