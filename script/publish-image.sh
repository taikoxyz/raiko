#!/usr/bin/env bash
set -x
set -eo pipefail

sgx_flags=$1
if [[ -n "$sgx_flags" ]]; then
	build_flags="${build_flags} --build-arg EDMM=${sgx_flags}"
fi
echo "sgx_flag=$sgx_flag"

if [[ -n $2 ]]; then
	tag=$2
else
	read -p "Do you have specific tag to build? default[latest]: " tag
	case "$tag" in
	"")
		tag=latest
		;;
	*) ;;
	esac
fi
echo "build tag is $tag"

# docker build
read -p "Do you want to build tee(0) or zk(1): " proof_type
case "$proof_type" in
0 | tee)
	image_name=raiko
	target_dockerfile=Dockerfile
	;;
1 | zk)
	image_name=raiko-zk
	target_dockerfile=Dockerfile.zk
	;;
*)
	echo "unknown proof type to build"
	exit 1
	;;
esac

echo "Build and push $image_name:$tag..."
docker buildx build . \
	-f $target_dockerfile \
	--load \
	--platform linux/amd64 \
	-t $image_name:$tag \
	$build_flags \
	--build-arg TARGETPLATFORM=linux/amd64 \
	--progress=plain \
	2>&1 | tee log.build.$image_name.$tag

# check docker build status
if [ $? -ne 0 ]; then
	echo "❌ Docker build failed!"
	exit 1
fi

DOCKER_REPOSITORY=us-docker.pkg.dev/evmchain/images
docker tag $image_name:$tag $DOCKER_REPOSITORY/$image_name:$tag

read -p "Do you want to push $image_name:$tag to registry? (y/N) " confirm
case "$confirm" in
[yY][eE][sS] | [yY])
	docker push $DOCKER_REPOSITORY/$image_name:$tag
	;;
*)
	echo "⏭️ Skipped: docker push $DOCKER_REPOSITORY/$image_name:$tag."
	echo "you can do it manually"
	;;
esac

echo "Done"
