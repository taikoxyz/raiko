#!/usr/bin/env bash

if [[ $# -eq 1 && $1 == "--init" ]]; then
    cd /opt/raiko/guests/sgx && gramine-sgx ./raiko-guest bootstrap
else
    /opt/raiko/bin/raiko-host "$@"
fi
