#!/bin/bash

echo "choose env"
select net in tolba hekla mainnet devnet others; do
  case $net in
    tolba|hekla|mainnet|devnet)
      network=$net
      break
      ;;
    others)
      read -p "Input customized env: " custom_net
      network=$custom_net
      break
      ;;
    *)
      echo "unknown option"
      ;;
  esac
done

# input version                                                                                                                                                                                               
read -p "Input version（e.g., 1.9.0-rc.1-edmm): " version

# check base directory exists
base_dir=${network}/${version}
if [ ! -d "$base_dir" ]; then
  echo "❌ Directory $base_dir does not exist. Please run the prepare-deploy.sh script first."
  exit 1
fi

TARGET_BASE_DIR="$network/$version"
NEW_SGX=$(jq -r '.sgx.instance_ids.PACAYA // 3131899904' $TARGET_BASE_DIR/raiko/config/config.sgx.json)
NEW_SGXGETH=$(jq -r '.sgxgeth.instance_ids.PACAYA // 3131899904' $TARGET_BASE_DIR/raiko/config/config.sgx.json)
NEW_SGXSHASTA=$(jq -r '.sgx.instance_ids.SHASTA // 3131899904' $TARGET_BASE_DIR/raiko/config/config.sgx.json)
NEW_SGXGETHSHASTA=$(jq -r '.sgxgeth.instance_ids.SHASTA // 3131899904' $TARGET_BASE_DIR/raiko/config/config.sgx.json)

echo "update env for sgx ids:"
# .env 
ENV_FILE=".env.$network.remote-sgx"
echo 'update env file: $ENV_FILE'
TARGET_FILE=".env.${network}.remote-sgx.${version}"
cp "$ENV_FILE" "$TARGET_FILE"

# replace
sed -i -E "s/^SGX_PACAYA_INSTANCE_ID=.*/SGX_PACAYA_INSTANCE_ID=${NEW_SGX}/" "$TARGET_FILE"
sed -i -E "s/^SGXGETH_PACAYA_INSTANCE_ID=.*/SGXGETH_PACAYA_INSTANCE_ID=${NEW_SGXGETH}/" "$TARGET_FILE"
sed -i -E "s/^SGX_SHASTA_INSTANCE_ID=.*/SGX_SHASTA_INSTANCE_ID=${NEW_SGXSHASTA}/" "$TARGET_FILE"
sed -i -E "s/^SGXGETH_SHASTA_INSTANCE_ID=.*/SGXGETH_SHASTA_INSTANCE_ID=${NEW_SGXGETHSHASTA}/" "$TARGET_FILE"

echo "Updated .env file:"
grep "SGX_PACAYA_INSTANCE_ID\|SGXGETH_PACAYA_INSTANCE_ID\|SGX_SHASTA_INSTANCE_ID\|SGXGETH_SHASTA_INSTANCE_ID" "$TARGET_FILE"

base_dir=${network}/${version}
echo "then run: \n"
echo "${network^^}_HOME=./${base_dir} docker compose -f docker-compose-${network}-${version}.yml --env-file ${TARGET_FILE} up ${network}-raiko-sgx-server -d"
