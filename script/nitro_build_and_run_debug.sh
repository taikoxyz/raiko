#!/bin/bash
echo Building prover image
docker build -f provers/nitro/nitro-prover/Dockerfile . -t raiko-prover

echo Generating EFI enclave
nitro-cli build-enclave --docker-uri raiko-prover:latest --output-file raiko-prover.efi

echo Running dev enclave
nitro-cli run-enclave --cpu-count 2 --memory 1024 --enclave-cid 16 --eif-path raiko-prover.eif --debug-mode --attach-console