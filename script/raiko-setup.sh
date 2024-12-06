#!/bin/bash

apt-get -qq install -y cpuid git build-essential wget python-is-python3 debhelper zip libcurl4-openssl-dev pkgconf libboost-dev libboost-system-dev libboost-thread-dev protobuf-c-compiler libprotobuf-c-dev protobuf-compiler

count=$(cpuid | grep -ic "SGX: Software Guard Extensions supported = true")

if [ $count -lt 1 ]
then
        echo "This machine does not have SGX support"
        exit 1
fi

linux_ver=$(uname -r | grep -ic "6.*")

if [ $linux_ver -lt 1 ]
then
        echo "Please ensure that your Linux kernel version is `6.0` or above."
        exit 1
fi

echo "deb [arch=amd64] https://download.01.org/intel-sgx/sgx_repo/ubuntu focal main" | tee /etc/apt/sources.list.d/intel-sgx.list > /dev/null

wget -q -O - https://download.01.org/intel-sgx/sgx_repo/ubuntu/intel-sgx-deb.key | apt-key add -

apt -qq update

apt -qq install sgx-pck-id-retrieval-tool

echo "Please enter your Intel PCS Service API key"

read -r API_KEY

PCKIDRetrievalTool -f /tmp/pckid.csv && pckid=$(cat /tmp/pckid.csv) && ppid=$(echo "$pckid" | awk -F "," '{print $1}') && cpusvn=$(echo "$pckid" | awk -F "," '{print $3}') && pcesvn=$(echo "$pckid" | awk -F "," '{print $4}') && pceid=$(echo "$pckid" | awk -F "," '{print $2}') && curl -v "https://api.trustedservices.intel.com/sgx/certification/v4/pckcert?encrypted_ppid=${ppid}&cpusvn=${cpusvn}&pcesvn=${pcesvn}&pceid=${pceid}" -H "Ocp-Apim-Subscription-Key:${API_KEY}" 2>&1 | grep -i "SGX-FMSPC"

echo "If your FMSPC is not on the list, please create a GitHub issue to have it added. If not, you will not be able to run Raiko."

curl -fsSL https://get.pnpm.io/install.sh | sh -
curl -L https://foundry.paradigm.xyz | bash
