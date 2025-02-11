#!/usr/bin/env bash
set -x
set -eo pipefail

sgx_flags=$1
if [[ -n "$sgx_flags" ]]; then
	build_flags="${build_flags} --build-arg EDMM=${sgx_flags}"
fi

tag=$2

if [[ -z "$tag" ]]; then
	tag="latest"
fi

echo "Build and push $1:$tag..."
docker buildx build ./ \
	--load \
	--platform linux/amd64 \
	-t raiko:$tag \
	$build_flags \
	--build-arg TARGETPLATFORM=linux/amd64 \
	--progress=plain

docker tag raiko:$tag us-docker.pkg.dev/evmchain/images/raiko:$tag

echo "Done"
