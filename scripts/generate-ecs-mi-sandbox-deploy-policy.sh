#!/bin/sh
set -eu
umask 077

if [ "$#" -ne 2 ]; then
    echo "usage: generate-ecs-mi-sandbox-deploy-policy.sh ABSOLUTE_PLAN_JSON ABSOLUTE_POLICY_OUTPUT" >&2
    exit 2
fi

plan_json=$1
policy_output=$2

case "$plan_json" in
    /*) ;;
    *)
        echo "plan JSON path must be absolute" >&2
        exit 2
        ;;
esac

case "$policy_output" in
    /*) ;;
    *)
        echo "policy output path must be absolute" >&2
        exit 2
        ;;
esac

if [ ! -f "$plan_json" ]; then
    echo "plan JSON file does not exist" >&2
    exit 1
fi

if [ -e "$policy_output" ]; then
    echo "refusing to overwrite an existing policy output" >&2
    exit 1
fi

if [ -z "${AWS_REGION:-}" ] || [ -z "${AWS_ACCOUNT_ID:-}" ]; then
    echo "set AWS_REGION and AWS_ACCOUNT_ID to the reviewed plan's target" >&2
    exit 2
fi

command -v uvx >/dev/null 2>&1 || {
    echo "uvx is required for IAM Policy Autopilot" >&2
    exit 1
}

temporary_output=$(mktemp "${policy_output}.XXXXXX")
trap 'rm -f "$temporary_output"' EXIT HUP INT TERM
chmod 600 "$temporary_output"

uvx iam-policy-autopilot@latest generate-policies \
    "$plan_json" \
    --region "$AWS_REGION" \
    --account "$AWS_ACCOUNT_ID" \
    --pretty \
    > "$temporary_output"

if [ ! -s "$temporary_output" ]; then
    echo "IAM Policy Autopilot returned an empty policy" >&2
    exit 1
fi

if ! ln "$temporary_output" "$policy_output"; then
    echo "could not create policy output without overwriting a file" >&2
    exit 1
fi

rm -f "$temporary_output"
trap - EXIT HUP INT TERM
chmod 600 "$policy_output"
echo "Wrote owner-only generated policy for review; it was not uploaded or applied."
