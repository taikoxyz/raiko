#!/bin/bash

NEW_SGX=$(jq -r '.sgx.instance_ids.PACAYA' hekla/0527/raiko/config/config.sgx.json)
NEW_SGXGETH=$(jq -r '.sgxgeth.instance_ids.PACAYA' hekla/0527/raiko/config/config.sgx.json)

echo "choose env"
select net in hekla mainnet devnet others; do
  case $net in
    hekla|mainnet|devnet)
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

# .env 
ENV_FILE=".env.$network.remote-sgx"
echo 'update env file: $ENV_FILE'

# replace
sed -i -E "s/^SGX_PACAYA_INSTANCE_ID=.*/SGX_PACAYA_INSTANCE_ID=${NEW_SGX}/" "$ENV_FILE"
sed -i -E "s/^SGXGETH_PACAYA_INSTANCE_ID=.*/SGXGETH_PACAYA_INSTANCE_ID=${NEW_SGXGETH}/" "$ENV_FILE"

echo "Updated .env file:"
grep "SGX_PACAYA_INSTANCE_ID\|SGXGETH_PACAYA_INSTANCE_ID" "$ENV_FILE"
