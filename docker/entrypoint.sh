#!/usr/bin/env bash

exec 2>&1
set -xeo pipefail

export IN_CONTAINER=1

# the config file & chain spec used inside raiko
BASE_CONFIG_FILE=${BASE_CONFIG_FILE:-config.sgx.json}
BASE_CHAINSPEC_FILE=${BASE_CHAINSPEC_FILE:-chain_spec_list.docker.json}
RAIKO_DOCKER_VOLUME_PATH=${RAIKO_DOCKER_VOLUME_PATH:-"/root/.config/raiko"}
RAIKO_DOCKER_VOLUME_CONFIG_PATH="$RAIKO_DOCKER_VOLUME_PATH/config"
RAIKO_DOCKER_VOLUME_SECRETS_PATH="$RAIKO_DOCKER_VOLUME_PATH/secrets"
RAIKO_DOCKER_VOLUME_PRIV_KEY_PATH="$RAIKO_DOCKER_VOLUME_SECRETS_PATH/priv.key"
RAIKO_APP_DIR=${RAIKO_APP_DIR:-"/opt/raiko/bin"}
RAIKO_CONF_DIR=${RAIKO_CONF_DIR:-"/etc/raiko"}
RAIKO_CONF_BASE_CONFIG="$RAIKO_CONF_DIR/$BASE_CONFIG_FILE"
RAIKO_CONF_CHAIN_SPECS="$RAIKO_CONF_DIR/$BASE_CHAINSPEC_FILE"
RAIKO_GUEST_APP_FILENAME="sgx-guest"
GAIKO_GUEST_APP_FILENAME="gaiko"
RAIKO_INPUT_MANIFEST_FILENAME="$RAIKO_GUEST_APP_FILENAME.docker.manifest.template"
RAIKO_OUTPUT_MANIFEST_FILENAME="$RAIKO_GUEST_APP_FILENAME.manifest.sgx"
RAIKO_SIGNED_MANIFEST_FILENAME="$RAIKO_GUEST_APP_FILENAME.sig"
GAIKO_GUEST_APP_VERBOSE_LEVEL=${GAIKO_GUEST_APP_VERBOSE_LEVEL:-3}

function sign_gramine_manifest() {
    cd "$RAIKO_APP_DIR"
    echo "Signing SGX manifest for current hardware..."
    
    # Check if manifest template exists
    if [[ ! -f "sgx-guest.manifest" ]]; then
        echo "sgx-guest.manifest not found. Cannot sign SGX enclave."
        return 1
    fi
    
    # Sign the manifest for current hardware
    gramine-sgx-sign --manifest "sgx-guest.manifest" --output "sgx-guest.manifest.sgx"
    
    if [[ $? -eq 0 ]]; then
        echo "SGX manifest signed successfully for current hardware"
        # Display enclave info for verification
        gramine-sgx-sigstruct-view "sgx-guest.sig"
        
        # Copy signed files to volume for persistence
        mkdir -p "$RAIKO_DOCKER_VOLUME_CONFIG_PATH"
        cp "sgx-guest.manifest.sgx" "sgx-guest.sig" "$RAIKO_DOCKER_VOLUME_CONFIG_PATH"
    else
        echo "Failed to sign SGX manifest"
        return 1
    fi
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





# Convenience function to update raiko config with SGX instance ids from env vars
function update_raiko_sgx_instance_id() {
    CONFIG_FILE=$1
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

function update_raiko_tdx_instance_id() {
    CONFIG_FILE=$1
    if [[ -n $TDX_INSTANCE_ID ]]; then
        jq \
            --arg update_value "$TDX_INSTANCE_ID" \
            '.tdx.instance_ids.HEKLA = ($update_value | tonumber)' $CONFIG_FILE \
            >/tmp/config_tmp.json && mv /tmp/config_tmp.json $CONFIG_FILE
        echo "Update hekla tdx instance id to $TDX_INSTANCE_ID"
    fi
    if [[ -n $TDX_ONTAKE_INSTANCE_ID ]]; then
        jq \
            --arg update_value "$TDX_ONTAKE_INSTANCE_ID" \
            '.tdx.instance_ids.ONTAKE = ($update_value | tonumber)' $CONFIG_FILE \
            >/tmp/config_tmp.json && mv /tmp/config_tmp.json $CONFIG_FILE
        echo "Update ontake tdx instance id to $TDX_ONTAKE_INSTANCE_ID"
    fi
    if [[ -n $TDX_PACAYA_INSTANCE_ID ]]; then
        jq \
            --arg update_value "$TDX_PACAYA_INSTANCE_ID" \
            '.tdx.instance_ids.PACAYA = ($update_value | tonumber)' $CONFIG_FILE \
            >/tmp/config_tmp.json && mv /tmp/config_tmp.json $CONFIG_FILE
        echo "Update pacaya tdx instance id to $TDX_PACAYA_INSTANCE_ID"
    fi
    if [[ -n $TDX_SHASTA_INSTANCE_ID ]]; then
        jq \
            --arg update_value "$TDX_SHASTA_INSTANCE_ID" \
            '.tdx.instance_ids.SHASTA = ($update_value | tonumber)' $CONFIG_FILE \
            >/tmp/config_tmp.json && mv /tmp/config_tmp.json $CONFIG_FILE
        echo "Update shasta tdx instance id to $TDX_SHASTA_INSTANCE_ID"
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
    echo "Running in SGX mode - signing enclave for current hardware"
    
    # Sign SGX manifest at runtime for current hardware
    sign_gramine_manifest
    
    if [[ $# -eq 1 && $1 == "--init" ]]; then
        echo "start bootstrap"
        bootstrap
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
        update_raiko_sgx_instance_id $RAIKO_CONF_BASE_CONFIG

        /opt/raiko/bin/raiko-host --config-path=$RAIKO_CONF_BASE_CONFIG --chain-spec-path=$RAIKO_CONF_CHAIN_SPECS "$@"
    fi
fi

if [[ -n $ZK ]]; then
    echo "running raiko in zk mode"
    if [ ! -f $RAIKO_CONF_BASE_CONFIG ]; then
        echo "$RAIKO_CONF_BASE_CONFIG file not found."
        exit 1
    fi
    /opt/raiko/bin/raiko-host  --config-path=$RAIKO_CONF_BASE_CONFIG --chain-spec-path=$RAIKO_CONF_CHAIN_SPECS "$@"
fi

if [[ -n $TDX ]]; then
    echo "running raiko in tdx mode"
    if [ ! -f $RAIKO_CONF_BASE_CONFIG ]; then
        echo "$RAIKO_CONF_BASE_CONFIG file not found."
        exit 1
    fi

    update_raiko_tdx_instance_id $RAIKO_CONF_BASE_CONFIG

    /opt/raiko/bin/raiko-host  --config-path=$RAIKO_CONF_BASE_CONFIG --chain-spec-path=$RAIKO_CONF_CHAIN_SPECS "$@"
fi
