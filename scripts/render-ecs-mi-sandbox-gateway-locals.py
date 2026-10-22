#!/usr/bin/env python3
"""Render non-secret gateway example locals from the sandbox's OpenTofu outputs."""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from pathlib import Path
from typing import Any


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
BASE_ROOT = REPOSITORY_ROOT / "examples/opentofu/ecs-managed-instances-sandbox"
GATEWAY_ROOT = REPOSITORY_ROOT / "examples/opentofu/ecs-managed-instances"
OUTPUT_FILE = GATEWAY_ROOT / "locals.auto.tf"


def output_value(outputs: dict[str, Any], name: str) -> Any:
    value = outputs.get(name)
    if not isinstance(value, dict) or "value" not in value:
        raise ValueError(f"required OpenTofu output is missing: {name}")
    return value["value"]


def hcl(value: Any) -> str:
    return json.dumps(value, ensure_ascii=True, separators=(", ", ": "))


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: render-ecs-mi-sandbox-gateway-locals.py sha256:<64-hex-digest>", file=sys.stderr)
        return 2

    image_digest = sys.argv[1]
    if re.fullmatch(r"sha256:[0-9a-f]{64}", image_digest) is None:
        print("image digest must be sha256 followed by 64 lowercase hexadecimal characters", file=sys.stderr)
        return 2

    try:
        result = subprocess.run(
            ["tofu", f"-chdir={BASE_ROOT}", "output", "-json"],
            check=True,
            capture_output=True,
            text=True,
        )
        outputs = json.loads(result.stdout)
    except (OSError, subprocess.CalledProcessError, json.JSONDecodeError) as error:
        print(f"could not read the sandbox's non-sensitive OpenTofu outputs: {error}", file=sys.stderr)
        return 1

    secret_arns = output_value(outputs, "gateway_secret_arns")
    test_urls = output_value(outputs, "private_test_urls")
    values = {
        "aws_region": output_value(outputs, "aws_region"),
        "name": output_value(outputs, "gateway_name"),
        "environment": "sandbox",
        "sandbox_id": output_value(outputs, "sandbox_id"),
        "image_uri": f"{output_value(outputs, 'gateway_ecr_repository_url')}@{image_digest}",
        "private_subnet_ids": output_value(outputs, "private_subnet_ids"),
        "managed_instance_security_group_ids": [
            output_value(outputs, "managed_instance_security_group_id")
        ],
        "task_security_group_ids": [output_value(outputs, "task_security_group_id")],
        "private_alb_target_group_arn": output_value(outputs, "gateway_target_group_arn"),
        "control_database_url_secret_arn": secret_arns["control_database_url"],
        "vault_admin_token_secret_arn": secret_arns["vault_admin_token"],
        "vault_runtime_token_secret_arn": secret_arns["vault_runtime_token"],
        "secret_kms_key_arns": [],
        "oidc_issuer_url": f"{test_urls['auth']}/application/o/sql-mcp-gateway-dev/",
        "oidc_client_id": "sql-mcp-gateway-dev",
        "oidc_access_token_audience": "sql-mcp-gateway-dev",
        "oidc_bootstrap_admin_subjects": ["dev-admin"],
        "vault_address": test_urls["vault"],
        "max_query_rows": 200,
        "max_query_duration_seconds": 30,
        "task_cpu": 512,
        "task_memory_mib": 1024,
        "cpu_architecture": "X86_64",
        "instance_storage_size_gib": 30,
        "allowed_instance_types": ["c7i.large"],
        "desired_count": 2,
        "log_retention_in_days": 1,
    }

    lines = ["# Generated from the temporary sandbox outputs. Do not add secret values.", "locals {"]
    for name, value in values.items():
        lines.append(f"  {name} = {hcl(value)}")
    lines.append("}")
    content = "\n".join(lines) + "\n"

    file_descriptor: int | None = None
    try:
        file_descriptor = os.open(OUTPUT_FILE, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(file_descriptor, "w", encoding="utf-8") as output_file:
            file_descriptor = None
            output_file.write(content)
            output_file.flush()
            os.fsync(output_file.fileno())
    except FileExistsError:
        print(f"refusing to overwrite existing file: {OUTPUT_FILE}", file=sys.stderr)
        return 1
    except OSError as error:
        if file_descriptor is not None:
            os.close(file_descriptor)
        print(f"could not write {OUTPUT_FILE}: {error}", file=sys.stderr)
        return 1

    print(f"Wrote owner-only non-secret gateway configuration: {OUTPUT_FILE}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
