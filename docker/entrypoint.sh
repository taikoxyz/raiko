#!/usr/bin/env bash

set -xeo pipefail

GRAMINE_PRIV_KEY="$HOME/.config/gramine/enclave-key.pem"
RAIKO_DOCKER_VOLUME_PATH="/root/.config/raiko"
RAIKO_DOCKER_VOLUME_CONFIG_PATH="$RAIKO_DOCKER_VOLUME_PATH/config"
RAIKO_DOCKER_VOLUME_SECRETS_PATH="$RAIKO_DOCKER_VOLUME_PATH/secrets"
RAIKO_DOCKER_VOLUME_PRIV_KEY_PATH="$RAIKO_DOCKER_VOLUME_SECRETS_PATH/priv.key"
RAIKO_APP_DIR="/opt/raiko/bin"
RAIKO_CONF_DIR="/etc/raiko"
RAIKO_GUEST_APP_FILENAME="sgx-guest"
RAIKO_GUEST_SETUP_FILENAME="raiko-setup"
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

function bootstrap_with_self_register() {
    mkdir -p "$RAIKO_DOCKER_VOLUME_SECRETS_PATH"
    cd "$RAIKO_APP_DIR"
    echo "./$RAIKO_GUEST_SETUP_FILENAME bootstrap --l1-rpc $L1_RPC --l1-chain-id $L1_CHAIN_ID --sgx-verifier-address $SGX_VERIFIER_ADDRESS"
    ./$RAIKO_GUEST_SETUP_FILENAME bootstrap --l1-rpc $L1_RPC --l1-chain-id $L1_CHAIN_ID --sgx-verifier-address $SGX_VERIFIER_ADDRESS
    cd -
}

function update_docker_chain_specs() {
    CONFIG_FILE="$RAIKO_CONF_DIR/chain_spec_list.docker.json"
    if [ ! -f $CONFIG_FILE ]; then
        echo "chain_spec_list.docker.json file not found."
        return 1
    fi

    if [ -n "${HOLESKY_RPC}" ]; then
        jq --arg rpc "$HOLESKY_RPC" 'map(if .name == "holesky" then .rpc = $rpc else . end)' $CONFIG_FILE > /tmp/config_tmp.json && mv /tmp/config_tmp.json $CONFIG_FILE;
        echo "Updated config.json with .rpc=$HOLESKY_RPC"
    fi

    if [ -n "${HOLESKY_BEACON_RPC}" ]; then
        jq --arg beacon_rpc "$HOLESKY_L1_BEACON_RPC" 'map(if .name == "holesky" then .beacon_rpc = $beacon_rpc else . end)' $CONFIG_FILE > /tmp/config_tmp.json && mv /tmp/config_tmp.json $CONFIG_FILE;
        echo "Updated config.json with .beacon_rpc=$HOLESKY_L1_BEACON_RPC"
    fi

    if [ -n "${TAIKO_A7_RPC}" ]; then
        jq --arg taiko_a7_rpc "$TAIKO_A7_RPC" 'map(if .name == "taiko_a7" then .rpc = $taiko_a7_rpc else . end)' $CONFIG_FILE > /tmp/config_tmp.json && mv /tmp/config_tmp.json $CONFIG_FILE;
        echo "Updated config.json with .taiko_a7_rpc=$TAIKO_A7_RPC"
    fi
}

if [[ -z "${PCCS_HOST}" ]]; then
    MY_PCCS_HOST=pccs:8081
else
    MY_PCCS_HOST=${PCCS_HOST}
fi

sed -i "s/https:\/\/localhost:8081/https:\/\/${MY_PCCS_HOST}/g" /etc/sgx_default_qcnl.conf
/restart_aesm.sh

echo $#
if [[ $# -eq 1 && $1 == "--init" ]]; then
    echo "start bootstrap"
    bootstrap
elif [[ $# -eq 1 && $1 == "--init-self-register" ]]; then
    echo "start bootstrap with self register"
    bootstrap_with_self_register
else
    echo "start proving"
    if [[ ! -f "$RAIKO_DOCKER_VOLUME_PRIV_KEY_PATH" ]]; then
        echo "Application was not bootstrapped. "\
             "$RAIKO_DOCKER_VOLUME_PRIV_KEY_PATH is missing. Bootstrap it first." >&2
        exit 1
    fi

    if [[ ! -z $SGX_INSTANCE_ID ]]; then
        echo "sed -i "s/123456/${SGX_INSTANCE_ID}/" /etc/raiko/config.sgx.json"
        sed -i "s/123456/${SGX_INSTANCE_ID}/" /etc/raiko/config.sgx.json
    fi

    update_docker_chain_specs

    /opt/raiko/bin/raiko-host "$@"
fi
