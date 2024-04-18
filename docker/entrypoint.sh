#!/usr/bin/env bash

set -xeo pipefail

GRAMINE_PRIV_KEY="$HOME/.config/gramine/enclave-key.pem"
RAIKO_DOCKER_VOLUME_PATH="/root/.config/raiko"
RAIKO_DOCKER_VOLUME_CONFIG_PATH="$RAIKO_DOCKER_VOLUME_PATH/config"
RAIKO_DOCKER_VOLUME_SECRETS_PATH="$RAIKO_DOCKER_VOLUME_PATH/secrets"
RAIKO_DOCKER_VOLUME_PRIV_KEY_PATH="$RAIKO_DOCKER_VOLUME_SECRETS_PATH/priv.key"
RAIKO_APP_DIR="/opt/raiko/bin"
RAIKO_GUEST_APP_FILENAME="sgx-guest"
RAIKO_INPUT_MANIFEST_FILENAME="$RAIKO_GUEST_APP_FILENAME.docker.manifest.template"
RAIKO_OUTPUT_MANIFEST_FILENAME="$RAIKO_GUEST_APP_FILENAME.manifest.sgx"
RAIKO_SIGNED_MANIFEST_FILENAME="$RAIKO_GUEST_APP_FILENAME.sig"

function sign_gramine_manifest() {
    cd "$RAIKO_APP_DIR"
    gramine-sgx-sign --manifest "$RAIKO_INPUT_MANIFEST_FILENAME" --output "$RAIKO_OUTPUT_MANIFEST_FILENAME"
    mkdir -p "$RAIKO_DOCKER_VOLUME_CONFIG_PATH"
    cp "$RAIKO_OUTPUT_MANIFEST_FILENAME" "$RAIKO_SIGNED_MANIFEST_FILENAME" "$RAIKO_DOCKER_VOLUME_CONFIG_PATH"
    cd -
}

function bootstrap() {
    mkdir -p "$RAIKO_DOCKER_VOLUME_SECRETS_PATH"
    cd "$RAIKO_APP_DIR"
    gramine-sgx "$RAIKO_GUEST_APP_FILENAME" bootstrap
    cd -
}

if [[ -z "${PCCS_HOST}" ]]; then
    MY_PCCS_HOST=pccs:8081
else
    MY_PCCS_HOST=${PCCS_HOST}
fi

sed -i "s/https:\/\/localhost:8081/https:\/\/${MY_PCCS_HOST}/g" /etc/sgx_default_qcnl.conf
sed -i "s/123456/${SGX_INSTANCE_ID}/" /etc/raiko/config.sgx.json
/restart_aesm.sh

echo $#
if [[ $# -eq 1 && $1 == "--init" ]]; then
    echo "start bootstrap"
    bootstrap
else
    echo "start proving"
    if [[ ! -f "$RAIKO_DOCKER_VOLUME_PRIV_KEY_PATH" ]]; then
        echo "Application was not bootstrapped. "\
             "$RAIKO_DOCKER_VOLUME_PRIV_KEY_PATH is missing. Bootstrap it first." >&2
        exit 1
    fi

    /opt/raiko/bin/raiko-host "$@"
fi
