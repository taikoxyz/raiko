#!/usr/bin/env bash

exec 2>&1
set -xeo pipefail

export IN_CONTAINER=1

# the config file & chain spec used inside raiko
BASE_CONFIG_FILE=${BASE_CONFIG_FILE:-config.sgx.json}
BASE_CHAINSPEC_FILE=${BASE_CHAINSPEC_FILE:-chain_spec_list.docker.json}
RAIKO_DOCKER_VOLUME_PATH="/root/.config/raiko"
RAIKO_DOCKER_VOLUME_CONFIG_PATH="$RAIKO_DOCKER_VOLUME_PATH/config"
RAIKO_DOCKER_VOLUME_SECRETS_PATH="$RAIKO_DOCKER_VOLUME_PATH/secrets"
RAIKO_DOCKER_VOLUME_PRIV_KEY_PATH="$RAIKO_DOCKER_VOLUME_SECRETS_PATH/priv.key"
RAIKO_APP_DIR="/opt/raiko/bin"
RAIKO_CONF_DIR="/etc/raiko"
RAIKO_CONF_BASE_CONFIG="$RAIKO_CONF_DIR/$BASE_CONFIG_FILE"
RAIKO_CONF_CHAIN_SPECS="$RAIKO_CONF_DIR/$BASE_CHAINSPEC_FILE"
DEVNET_CHAINSPEC_FILE=$RAIKO_CONF_DIR/chain_spec_list_devnet.json
PRODUCT_CHAINSPEC_FILE=$RAIKO_CONF_DIR/chain_spec_list_default.json
RAIKO_GUEST_APP_FILENAME="sgx-guest"
GAIKO_GUEST_APP_FILENAME="gaiko"
RAIKO_GUEST_SETUP_FILENAME="raiko-setup"
RAIKO_INPUT_MANIFEST_FILENAME="$RAIKO_GUEST_APP_FILENAME.docker.manifest.template"
RAIKO_OUTPUT_MANIFEST_FILENAME="$RAIKO_GUEST_APP_FILENAME.manifest.sgx"
RAIKO_SIGNED_MANIFEST_FILENAME="$RAIKO_GUEST_APP_FILENAME.sig"
GAIKO_GUEST_APP_VERBOSE_LEVEL=${GAIKO_GUEST_APP_VERBOSE_LEVEL:-3}

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
    if [[ -n $SGXGETH ]]; then
        echo "bootstrap geth sgx prover"
        ./"$GAIKO_GUEST_APP_FILENAME" bootstrap
    fi

    echo "bootstrap sgx prover"
    gramine-sgx "$RAIKO_GUEST_APP_FILENAME" bootstrap
    cd -
}

function bootstrap_with_self_register() {
    L1_NETWORK="${L1_NETWORK:-holesky}"
    NETWORK="${NETWORK:-taiko_a7}"
    mkdir -p "$RAIKO_DOCKER_VOLUME_SECRETS_PATH"
    cd "$RAIKO_APP_DIR"
    ./$RAIKO_GUEST_SETUP_FILENAME bootstrap --l1-network $L1_NETWORK --network $NETWORK --chain-spec-path $RAIKO_CONF_CHAIN_SPECS
    cd -
}

function update_chain_spec_json() {
    CONFIG_FILE=$1
    CHAIN_NAME=$2
    KEY_NAME=$3
    UPDATE_VALUE=$4
    jq \
        --arg update_value "$UPDATE_VALUE" \
        --arg chain_name "$CHAIN_NAME" \
        --arg key_name "$KEY_NAME" \
        'map(if .name == $chain_name then .[$key_name] = $update_value else . end)' $CONFIG_FILE \
        >/tmp/config_tmp.json && mv /tmp/config_tmp.json $CONFIG_FILE
    echo "Updated $CONFIG_FILE $CHAIN_NAME.$KEY_NAME=$UPDATE_VALUE"
}

function update_docker_chain_specs() {
    CONFIG_FILE=$1
    if [ ! -f $CONFIG_FILE ]; then
        echo "$BASE_CHAINSPEC_FILE file not found."
        return 1
    fi

    if [ -n "${ETHEREUM_RPC}" ]; then
        update_chain_spec_json $CONFIG_FILE "ethereum" "rpc" $ETHEREUM_RPC
    fi

    if [ -n "${ETHEREUM_BEACON_RPC}" ]; then
        update_chain_spec_json $CONFIG_FILE "ethereum" "beacon_rpc" $ETHEREUM_BEACON_RPC
    fi

    if [ -n "${HOLESKY_RPC}" ]; then
        update_chain_spec_json $CONFIG_FILE "holesky" "rpc" $HOLESKY_RPC
    fi

    if [ -n "${HOLESKY_BEACON_RPC}" ]; then
        update_chain_spec_json $CONFIG_FILE "holesky" "beacon_rpc" $HOLESKY_BEACON_RPC
    fi

    if [ -n "${TAIKO_A7_RPC}" ]; then
        update_chain_spec_json $CONFIG_FILE "taiko_a7" "rpc" $TAIKO_A7_RPC
    fi

    if [ -n "${TAIKO_MAINNET_RPC}" ]; then
        update_chain_spec_json $CONFIG_FILE "taiko_mainnet" "rpc" $TAIKO_MAINNET_RPC
    fi

    if [ -n "${HOODI_RPC}" ]; then
        update_chain_spec_json $CONFIG_FILE "hoodi" "rpc" $HOODI_RPC
    fi

    if [ -n "${HOODI_BEACON_RPC}" ]; then
        update_chain_spec_json $CONFIG_FILE "hoodi" "beacon_rpc" $HOODI_BEACON_RPC
    fi

    if [ -n "${TOLBA_RPC}" ]; then
        update_chain_spec_json $CONFIG_FILE "taiko_hoodi" "rpc" $TOLBA_RPC
    fi
}

function update_config_json() {
    CONFIG_FILE=$1
    KEY_NAME=$2
    UPDATE_VALUE=$3
    jq \
        --arg update_value "$UPDATE_VALUE" \
        --arg key_name "$KEY_NAME" \
        '.[$key_name] = $update_value' $CONFIG_FILE \
        >/tmp/config_tmp.json && mv /tmp/config_tmp.json $CONFIG_FILE
    echo "Updated $CONFIG_FILE $CHAIN_NAME.$KEY_NAME=$UPDATE_VALUE"
}

function update_raiko_network() {
    CONFIG_FILE=$1
    if [ -n "${L1_NETWORK}" ]; then
        update_config_json $CONFIG_FILE "l1_network" $L1_NETWORK
    fi

    if [ -n "${NETWORK}" ]; then
        update_config_json $CONFIG_FILE "network" $NETWORK
    fi
}

function update_raiko_sgx_instance_id() {
    CONFIG_FILE=$1
    if [[ -n $SGX_INSTANCE_ID ]]; then
        jq \
            --arg update_value "$SGX_INSTANCE_ID" \
            '.sgx.instance_ids.HEKLA = ($update_value | tonumber)' $CONFIG_FILE \
            >/tmp/config_tmp.json && mv /tmp/config_tmp.json $CONFIG_FILE
        echo "Update hekla sgx instance id to $SGX_INSTANCE_ID"
    fi
    if [[ -n $SGX_ONTAKE_INSTANCE_ID ]]; then
        jq \
            --arg update_value "$SGX_ONTAKE_INSTANCE_ID" \
            '.sgx.instance_ids.ONTAKE = ($update_value | tonumber)' $CONFIG_FILE \
            >/tmp/config_tmp.json && mv /tmp/config_tmp.json $CONFIG_FILE
        echo "Update ontake sgx instance id to $SGX_ONTAKE_INSTANCE_ID"
    fi
    if [[ -n $SGX_PACAYA_INSTANCE_ID ]]; then
        jq \
            --arg update_value "$SGX_PACAYA_INSTANCE_ID" \
            '.sgx.instance_ids.PACAYA = ($update_value | tonumber)' $CONFIG_FILE \
            >/tmp/config_tmp.json && mv /tmp/config_tmp.json $CONFIG_FILE
        echo "Update pacaya sgx instance id to $SGX_PACAYA_INSTANCE_ID"
    fi
    if [[ -n $SGXGETH_PACAYA_INSTANCE_ID ]]; then
        jq \
            --arg update_value "$SGXGETH_PACAYA_INSTANCE_ID" \
            '.sgxgeth.instance_ids.PACAYA = ($update_value | tonumber)' $CONFIG_FILE \
            >/tmp/config_tmp.json && mv /tmp/config_tmp.json $CONFIG_FILE
        echo "Update pacaya sgxgeth instance id to $SGXGETH_PACAYA_INSTANCE_ID"
    fi
    if [[ -n $SGX_SHASTA_INSTANCE_ID ]]; then
        jq \
            --arg update_value "$SGX_SHASTA_INSTANCE_ID" \
            '.sgx.instance_ids.SHASTA = ($update_value | tonumber)' $CONFIG_FILE \
            >/tmp/config_tmp.json && mv /tmp/config_tmp.json $CONFIG_FILE
        echo "Update shasta sgx instance id to $SGX_SHASTA_INSTANCE_ID"
    fi
    if [[ -n $SGXGETH_SHASTA_INSTANCE_ID ]]; then
        jq \
            --arg update_value "$SGXGETH_SHASTA_INSTANCE_ID" \
            '.sgxgeth.instance_ids.SHASTA = ($update_value | tonumber)' $CONFIG_FILE \
            >/tmp/config_tmp.json && mv /tmp/config_tmp.json $CONFIG_FILE
        echo "Update shasta sgxgeth instance id to $SGXGETH_SHASTA_INSTANCE_ID"
    fi
}

# merge devnet & product chain spec here
function merge_json_arrays() {
    local input1="$1"
    local input2="$2"
    local output="$3"

    if [[ ! -f "$input1" || ! -f "$input2" ]]; then
        echo "❌ $input1 or $input2 not found！"
        return 1
    fi

    jq -s 'add' "$input1" "$input2" >"$output"

    if [[ $? -eq 0 ]]; then
        echo "✅ merge chainspec success to: $output"
    else
        echo "❌ merge failed!"
        return 1
    fi
}

if [[ -z "${PCCS_HOST}" ]]; then
    MY_PCCS_HOST=pccs:8081
else
    MY_PCCS_HOST=${PCCS_HOST}
fi

if [[ -n $TEST ]]; then
    echo "TEST mode, to test bash functions."
    return 0
fi

# sgx mode
if [[ -n $SGX || -n $SGX_SERVER ]]; then
    sed -i "s/https:\/\/localhost:8081/https:\/\/${MY_PCCS_HOST}/g" /etc/sgx_default_qcnl.conf
    /restart_aesm.sh
fi

echo $#
if [[ -n $SGX ]]; then
    if [[ $# -eq 1 && $1 == "--init" ]]; then
        echo "start bootstrap"
        bootstrap
    elif [[ $# -eq 1 && $1 == "--init-self-register" ]]; then
        echo "start bootstrap with self register"
        #merge chain spec to a all-in-one file
        merge_json_arrays $PRODUCT_CHAINSPEC_FILE $DEVNET_CHAINSPEC_FILE $RAIKO_CONF_CHAIN_SPECS
        update_docker_chain_specs $RAIKO_CONF_CHAIN_SPECS
        bootstrap_with_self_register
    else
        echo "start proving"
        if [ "$SGX_MODE" = "local" ]; then
            if [[ ! -f "$RAIKO_DOCKER_VOLUME_PRIV_KEY_PATH" ]]; then
                echo "Application was not bootstrapped. " \
                    "$RAIKO_DOCKER_VOLUME_PRIV_KEY_PATH is missing. Bootstrap it first." >&2
                exit 1
            fi
        fi

        if [ ! -f $RAIKO_CONF_BASE_CONFIG ]; then
            echo "$RAIKO_CONF_BASE_CONFIG file not found."
            exit 1
        fi

        #update raiko server config
        update_raiko_network $RAIKO_CONF_BASE_CONFIG
        update_raiko_sgx_instance_id $RAIKO_CONF_BASE_CONFIG

        #merge chain spec to a all-in-one file
        merge_json_arrays $PRODUCT_CHAINSPEC_FILE $DEVNET_CHAINSPEC_FILE $RAIKO_CONF_CHAIN_SPECS
        update_docker_chain_specs $RAIKO_CONF_CHAIN_SPECS

        /opt/raiko/bin/raiko-host --config-path=$RAIKO_CONF_BASE_CONFIG --chain-spec-path=$RAIKO_CONF_CHAIN_SPECS "$@"
    fi
fi

if [[ -n $ZK ]]; then
    echo "running raiko in zk mode"
    if [ ! -f $RAIKO_CONF_BASE_CONFIG ]; then
        echo "$RAIKO_CONF_BASE_CONFIG file not found."
        exit 1
    fi

    #update raiko server config
    update_raiko_network $RAIKO_CONF_BASE_CONFIG
    update_raiko_sgx_instance_id $RAIKO_CONF_BASE_CONFIG

    #update raiko server chainspec
    merge_json_arrays $PRODUCT_CHAINSPEC_FILE $DEVNET_CHAINSPEC_FILE $RAIKO_CONF_CHAIN_SPECS
    update_docker_chain_specs $RAIKO_CONF_CHAIN_SPECS

    /opt/raiko/bin/raiko-host --config-path=$RAIKO_CONF_BASE_CONFIG --chain-spec-path=$RAIKO_CONF_CHAIN_SPECS "$@"
fi

if [[ -n $SGX_SERVER ]]; then
    echo "running sgx in sgx server mode"

    if [[ $# -eq 1 && $1 == "--init" ]]; then
        echo "start server bootstrap"
        # useless here, as it can share same raiko init
        # keep it now for future refactory
        bootstrap
    else
        if [[ -z $SGX_PACAYA_INSTANCE_ID || -z $SGXGETH_PACAYA_INSTANCE_ID ]]; then
            echo "SGX_PACAYA_INSTANCE_ID and SGXGETH_PACAYA_INSTANCE_ID must be presented, please check."
            exit 1
        fi

        if [[ -z $SGX_SHASTA_INSTANCE_ID || -z $SGXGETH_SHASTA_INSTANCE_ID ]]; then
            echo "SGX_SHASTA_INSTANCE_ID and SGXGETH_SHASTA_INSTANCE_ID must be presented, please check."
            exit 1
        fi

        echo "Expanded instance IDs: {\"PACAYA\": ${SGX_PACAYA_INSTANCE_ID}, \"SHASTA\": ${SGX_SHASTA_INSTANCE_ID}}"
        echo "start sgx-guest --sgx-instance-ids='{\"PACAYA\": ${SGX_PACAYA_INSTANCE_ID}, \"SHASTA\": ${SGX_SHASTA_INSTANCE_ID}}' --address 0.0.0.0 --port 9090"
        gramine-sgx /opt/raiko/bin/sgx-guest serve --sgx-instance-ids="{\"PACAYA\": ${SGX_PACAYA_INSTANCE_ID}, \"SHASTA\": ${SGX_SHASTA_INSTANCE_ID}}" --address 0.0.0.0 --port 9090 | sed 's/^/[raiko] /' &

        echo "Expanded geth instance IDs: {\"PACAYA\": ${SGXGETH_PACAYA_INSTANCE_ID}, \"SHASTA\": ${SGXGETH_SHASTA_INSTANCE_ID}}"
        echo "start gaiko serve --sgx-instance-ids='{\"PACAYA\": ${SGXGETH_PACAYA_INSTANCE_ID}, \"SHASTA\": ${SGXGETH_SHASTA_INSTANCE_ID}}' --port 8090"
        /opt/raiko/bin/gaiko --verbosity $GAIKO_GUEST_APP_VERBOSE_LEVEL serve --sgx-instance-ids="{\"PACAYA\": ${SGXGETH_PACAYA_INSTANCE_ID}, \"SHASTA\": ${SGXGETH_SHASTA_INSTANCE_ID}}" --port 8090 | sed 's/^/[gaiko] /' &
        wait
    fi
fi
