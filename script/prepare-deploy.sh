#!/bin/bash

set -e

echo "choose env"
select net in hekla tolba mainnet devnet others; do
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
read -p "Input image version（e.g., 1.8.0-edmm): " version

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

#prepare version docker compose
original_file="docker-compose-${network}.yml"
release_file="docker-compose-${network}-${version}.yml"
sed "s|image: us-docker.pkg.dev/evmchain/images/raiko:latest|image: us-docker.pkg.dev/evmchain/images/raiko:${version}|g" "$original_file" > "$release_file"

echo " $release_file"
# pull docker image and check if any error occurs
echo "✅ Pulling docker image us-docker.pkg.dev/evmchain/images/raiko:${version}"
if ! docker pull us-docker.pkg.dev/evmchain/images/raiko:${version}; then
  echo "❌ Failed to pull the docker image. Please check the version or your network connection."
  exit 1
fi

echo "✅ Prepare deployment down:"
echo "Please export ${network^^}_HOME=./${deploy_name}"
echo "Run: \n"
echo "${network^^}_HOME=./${deploy_name} docker compose -f ${release_file} --env-file .env.${network}.remote-sgx up init-self-register"
echo "then run: \n"
echo "${network^^}_HOME=./${deploy_name} docker compose -f ${release_file} --env-file .env.${network}.remote-sgx up ${network}-raiko-sgx-server -d"

#tree "$network/$version"
