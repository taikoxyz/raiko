#!/bin/bash

if [ -z $1 ] ; then
    echo "Please provide the new sgx ID"
    exit 1
fi
NEW_ID=$1


FILTER_NAME="raiko"
CONTAINER_ID=$(docker ps --filter "name=$FILTER_NAME" --format "{{.ID}}")
echo "Ready to config container: $CONTAINER_ID"

# pre-check
echo "Old config"
docker exec $CONTAINER_ID cat /etc/raiko/config.sgx.json
echo
docker exec $CONTAINER_ID sed -i "s/123456/$NEW_ID/" /etc/raiko/config.sgx.json
# post-check update
echo "New config"
docker exec $CONTAINER_ID cat /etc/raiko/config.sgx.json