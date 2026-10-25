"""Resolve Lambda secret references, then replace this process with the gateway."""

from __future__ import annotations

import json
import os
import sys
from typing import Any

import boto3


GATEWAY_BINARY = "/usr/local/bin/sql-mcp-gateway"


class BootstrapError(Exception):
    """A safe-to-log startup error that never contains a resolved secret."""


def required_secret(client: Any, env_name: str) -> str:
    secret_id = os.environ.get(env_name)
    if not secret_id:
        raise BootstrapError(f"required secret reference {env_name} is not configured")

    try:
        response = client.get_secret_value(SecretId=secret_id)
    except Exception as error:
        raise BootstrapError(
            f"could not retrieve secret reference {env_name} ({type(error).__name__})"
        ) from None

    value = response.get("SecretString")
    if not isinstance(value, str) or not value.strip():
        raise BootstrapError(f"secret reference {env_name} has no non-empty SecretString")
    return value


def required_vault_token(secret: object, key: str) -> str:
    if not isinstance(secret, dict):
        raise BootstrapError("Vault credentials secret must be a JSON object")

    value = secret.get(key)
    if not isinstance(value, str) or not value.strip():
        raise BootstrapError(f"Vault credentials secret is missing the {key} field")
    return value


def main() -> None:
    try:
        client = boto3.client("secretsmanager")
        os.environ["CONTROL_DATABASE_URL"] = required_secret(
            client, "CONTROL_DATABASE_URL_SECRET_ARN"
        )
        os.environ["MCP_OAUTH_STATE_KEY"] = required_secret(
            client, "MCP_OAUTH_STATE_KEY_SECRET_ARN"
        )

        vault_secret_text = required_secret(client, "VAULT_CREDENTIALS_SECRET_ARN")
        try:
            vault_secret = json.loads(vault_secret_text)
        except json.JSONDecodeError:
            raise BootstrapError("Vault credentials secret is not valid JSON") from None

        admin_token = required_vault_token(vault_secret, "admin_token")
        runtime_token = required_vault_token(vault_secret, "runtime_token")
        if admin_token == runtime_token:
            raise BootstrapError("Vault administration and runtime tokens must be distinct")

        os.environ["VAULT_ADMIN_TOKEN"] = admin_token
        os.environ["VAULT_RUNTIME_TOKEN"] = runtime_token

        os.execv(GATEWAY_BINARY, [GATEWAY_BINARY])
    except BootstrapError as error:
        print(f"SQL MCP Gateway secret bootstrap failed: {error}", file=sys.stderr)
        raise SystemExit(1) from None
    except Exception as error:
        print(
            "SQL MCP Gateway bootstrap failed; check the runtime image and AWS "
            f"execution role ({type(error).__name__})",
            file=sys.stderr,
        )
        raise SystemExit(1) from None


if __name__ == "__main__":
    main()
