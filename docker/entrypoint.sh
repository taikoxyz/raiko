#!/usr/bin/env bash

set -xeo pipefail

GRAMINE_PRIV_KEY="$HOME/.config/gramine/enclave-key.pem"
RAIKO_DOCKER_VOLUME_PATH="/root/.config/raiko"
RAIKO_DOCKER_VOLUME_CONFIG_PATH="$RAIKO_DOCKER_VOLUME_PATH/config"
RAIKO_DOCKER_VOLUME_SECRETS_PATH="$RAIKO_DOCKER_VOLUME_PATH/secrets"
RAIKO_DOCKER_VOLUME_PRIV_KEY_PATH="$RAIKO_DOCKER_VOLUME_SECRETS_PATH/priv.key"
RAIKO_GUEST_APP_DIR="/opt/raiko/guests/sgx"
RAIKO_GUEST_APP_FILENAME="raiko-guest"
RAIKO_INPUT_MANIFEST_FILENAME="$RAIKO_GUEST_APP_FILENAME.manifest"
RAIKO_OUTPUT_MANIFEST_FILENAME="$RAIKO_GUEST_APP_FILENAME.manifest.sgx"
RAIKO_SIGNED_MANIFEST_FILENAME="$RAIKO_GUEST_APP_FILENAME.sig"

function sign_gramine_manifest() {
    cd "$RAIKO_GUEST_APP_DIR"
    gramine-sgx-sign --manifest "$RAIKO_INPUT_MANIFEST_FILENAME" --output "$RAIKO_OUTPUT_MANIFEST_FILENAME"
    mkdir -p "$RAIKO_DOCKER_VOLUME_CONFIG_PATH"
    cp "$RAIKO_OUTPUT_MANIFEST_FILENAME" "$RAIKO_SIGNED_MANIFEST_FILENAME" "$RAIKO_DOCKER_VOLUME_CONFIG_PATH"
    cd -
}

function bootstrap() {
    mkdir -p "$RAIKO_DOCKER_VOLUME_SECRETS_PATH"
    cd "$RAIKO_GUEST_APP_DIR"
    gramine-sgx "$RAIKO_GUEST_APP_FILENAME" bootstrap
    cd -
}

npm config set engine-strict true
cd /opt/intel/sgx-dcap-pccs
npm install
cd -

PCKIDRetrievalTool
/restart_aesm.sh
sleep 10
ls /opt/intel/sgx-dcap-pccs/config
ls /opt/intel/sgx-dcap-pccs/ssl_key/
cat /opt/intel/sgx-dcap-pccs/config/default.json
cd /opt/intel/sgx-dcap-pccs
node --version
# tree /opt/intel/sgx-dcap-pccs
# ls -la /opt/intel/sgx-dcap-pccs/node_modules/config
# ls: cannot access '/opt/intel/sgx-dcap-pccs/node_modules/config': No such file or directory
# node ./pccs_server.js &
# sleep 10
curl -v -k -G "https://host.docker.internal:8081/sgx/certification/v3/rootcacrl"
# sudo -u pccs node -r esm pccs_server.js
cd -

cat /etc/sgx_default_qcnl.conf
ls -la /dev/
ps -ef | grep -i pccs
ps -ef | grep -i aesm
ps -ef | grep -i sgx


if [[ $# -gt 0 && $1 == "--init" ]]; then
    if [[ ! -f "$GRAMINE_PRIV_KEY" ]]; then
        gramine-sgx-gen-private-key
    fi
    sign_gramine_manifest
    bootstrap
else
    if [[ ! -f "$RAIKO_DOCKER_VOLUME_PRIV_KEY_PATH" ]]; then
        echo "Application was not bootstrapped. "\
             "$RAIKO_DOCKER_VOLUME_PRIV_KEY_PATH is missing. Bootstrap it first." >&2
        exit 1
    fi

    sign_gramine_manifest
    /opt/raiko/bin/raiko-host "$@"
fi
