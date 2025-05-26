#!/bin/bash

set -e

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

# input version
read -p "Input version（e.g.：0522）: " version


deploy_name=${network}/${version}
# create directory
base_dir="${deploy_name}/raiko"

if [ -d "$base_dir" ]; then
  echo "❌ Found existing: $base_dir"
  # if user want to overwrite, remove the directory
  read -p "Do you want to overwrite? (y/n): " overwrite
  if [ "$overwrite" == "y" ]; then
    sudo rm -rf "$base_dir"
  else
    exit 1
  fi
fi

mkdir -p "$base_dir/config"
mkdir -p "$base_dir/secrets"
cp ../host/config/config.sgx.json "$base_dir/config/"

echo "✅ Prepare deployment down:"
echo "Please export ${network^^}_HOME=./${deploy_name}"
echo "Run: \n"
echo "${network^^}_HOME=./${deploy_name} docker compose -f docker-compose-${network}.yml --env-file .env.${network}.remote-sgx up init-self-register"
echo "then run: \n"
echo "${network^^}_HOME=./${deploy_name} docker compose -f docker-compose-${network}.yml --env-file .env.${network}.remote-sgx up ${network}-raiko-sgx-server -d"

#tree "$network/$version"
