#!/bin/sh
set -eu
umask 077

if [ "$#" -ne 2 ]; then
    echo "usage: prepare-ecs-mi-sandbox-build-context.sh AWS_REGION TLS_CA_PARAMETER_NAME" >&2
    exit 2
fi

aws_region=$1
ca_parameter_name=$2
build_directory=.dev/ecs-mi-sandbox
ca_file="$build_directory/tls-root-ca.crt"
rds_bundle_file="$build_directory/rds-global-bundle.pem"

command -v aws >/dev/null 2>&1 || {
    echo "AWS CLI is required to retrieve the public sandbox CA certificate" >&2
    exit 1
}
command -v curl >/dev/null 2>&1 || {
    echo "curl is required to retrieve the public RDS CA bundle" >&2
    exit 1
}

case "$ca_parameter_name" in
    /sqlgw-*/tls/root-ca) ;;
    *)
        echo "CA parameter name does not match the sandbox path format" >&2
        exit 2
        ;;
esac

if [ -L .dev ] || [ -L "$build_directory" ]; then
    echo "sandbox build path contains a symbolic link; refusing to use it" >&2
    exit 1
fi

if [ -e .dev ]; then
    if [ ! -d .dev ]; then
        echo ".dev exists but is not a directory; refusing to replace it" >&2
        exit 1
    fi
else
    mkdir -m 700 .dev
fi

if [ -e "$build_directory" ]; then
    if [ ! -d "$build_directory" ]; then
        echo "sandbox build path exists but is not a directory; refusing to replace it" >&2
        exit 1
    fi
else
    mkdir -m 700 "$build_directory"
fi

if [ -e "$ca_file" ] || [ -L "$ca_file" ] \
    || [ -e "$rds_bundle_file" ] || [ -L "$rds_bundle_file" ]; then
    echo "sandbox build certificate files already exist; refusing to overwrite them" >&2
    exit 1
fi

temporary_ca=$(mktemp "$build_directory/tls-root-ca.XXXXXX")
temporary_bundle=
cleanup() {
    rm -f "$temporary_ca"
    if [ -n "$temporary_bundle" ]; then
        rm -f "$temporary_bundle"
    fi
}
trap cleanup EXIT HUP INT TERM
temporary_bundle=$(mktemp "$build_directory/rds-global-bundle.XXXXXX")

aws ssm get-parameter \
    --name "$ca_parameter_name" \
    --query Parameter.Value \
    --output text \
    --region "$aws_region" \
    > "$temporary_ca"
curl --fail --silent --show-error --location \
    https://truststore.pki.rds.amazonaws.com/global/global-bundle.pem \
    --output "$temporary_bundle"

chmod 600 "$temporary_ca" "$temporary_bundle"
ln "$temporary_ca" "$ca_file"
if ! ln "$temporary_bundle" "$rds_bundle_file"; then
    rm -f "$ca_file"
    echo "could not create the RDS bundle without overwriting a file" >&2
    exit 1
fi
rm -f "$temporary_ca" "$temporary_bundle"
trap - EXIT HUP INT TERM
echo "Prepared owner-only public CA files for the sandbox image build."
