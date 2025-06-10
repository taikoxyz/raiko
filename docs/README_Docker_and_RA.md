# Raiko Docker Setup Tutorial

This tutorial was created to assist you in setting up Raiko and its SGX dependencies using a Docker container. Configuring SGX can be complex without a detailed guide to walk you through each step. This tutorial strives to provide a comprehensive walkthrough, leaving no detail unaddressed.

## Recommended Specs

We recommended 4 cores and 8GB memory for running Raiko. 8 cores and 16GB memory is ideal; the bare minimum is 2 cores and 4GB memory (tentative).

We also recommend an EPC (Enclave memory) size of 4GB for mainnet, to prevent OOM errors. You can check your machine's EPC size by running `./script/check-epc-size.sh`.

## Installing Dependencies

To make the process of setup a bit more straightforward, we've provided a script to install dependencies and check your machine's FMSPC in one go. Please prepare your Intel API Key before running.

```bash
cd raiko
sudo bash script/raiko-setup.sh
source ~/.bashrc
foundryup
```

The script does NOT include Docker as that is dependent on your distribution, please follow the docs to install the CLI.

After running this script your machine should be setup and you may skip to the `2. Generating PCCS Certificates` part of the guide and continue as normal.

## Prerequisites

Intel SGX is a technology that involves a considerable amount of configuration. Given its high level of configurability, the setup of your infrastructure may vary significantly depending on the attestation type (EPID, ECDSA) and other parameters. While we've strived to minimize the manual effort required to prepare the development environment, there are certain prerequisites that are challenging, if not impossible, to automate using Dockerfiles. This section outlines these prerequisites.

### Intel SGX-enabled CPU

Ensure that your machine has an [Intel SGX][sgx]-enabled CPU to run Raiko. You can verify if your CPU supports SGX (Software Guard Extensions) on Linux by using the [`cpuid`][cpuid] tool.

1. If `cpuid` isn't already installed, you can install it. On Ubuntu, use the following command:

        sudo apt-get install cpuid

2. Run `cpuid` and `grep` for `sgx`:

        cpuid | grep -i sgx

    If your CPU supports SGX, the output should resemble the following:

    ```
    SGX: Software Guard Extensions supported = true
    ```

    If this line doesn't appear, your CPU either doesn't support SGX, or it isn't enabled in the BIOS.

As an alternative, you can execute `grep sgx /proc/cpuinfo`. If the command doesn't return any output, your CPU doesn't support SGX.

[sgx]: https://www.intel.com/content/www/us/en/architecture-and-technology/software-guard-extensions.html
[cpuid]: https://manpages.ubuntu.com/manpages/noble/en/man1/cpuid.1.html

### Modern Linux kernel

Starting with Linux kernel version [`5.11`][kernel-5.11], the kernel provides built-in support for SGX. However, it doesn't support one of its latest features, [EDMM][edmm] (Enclave Dynamic Memory Management), which Raiko requires. EDMM support was first introduced in Linux `6.0`, so ensure that your Linux kernel version is `6.0` or above.

To check the version of your kernel, run:

```
uname -a
```

If you're using Ubuntu and want to see the available Linux kernel versions, run the following command:

```
apt search linux-image
```

Once you have determined the version of the kernel that you want to downgrade or upgrade, run the following command to install:

```
sudo apt-get install linux-image-{version}-generic
```

Then reboot the system

[kernel-5.11]: https://www.intel.com/content/www/us/en/developer/tools/software-guard-extensions/linux-overview.html
[edmm]: https://gramine.readthedocs.io/en/stable/manifest-syntax.html#edmm

### Subscribing to Intel PCS Service

To use ECDSA Attestation, you need to subscribe to the Intel PCS service, following the steps in [Intel's how-to guide][intel-dcap-install-howto]. After subscribing to the service, you will get two keys: a primary API key and a secondary API key.

[intel-dcap-install-howto]: https://www.intel.com/content/www/us/en/developer/articles/guide/intel-software-guard-extensions-data-center-attestation-primitives-quick-install-guide.html

> **_NOTE:_** You do NOT need to follow the entirety of the linked guide, just the `Subscribe to the Intel PCS` section.

### Verify that your SGX machine has a compatible FMSPC

At the moment Raiko only supports certain `fmspc`, so to prevent wasted time check if your machine is on our supported fmspc list.

To retrieve this information, you will need to use the `PCKIDRetrievalTool` and query the Intel API.

1. Install the `PCKIDRetrievalTool`

You can install either from the Ubuntu repository:
```
echo "deb [arch=amd64] https://download.01.org/intel-sgx/sgx_repo/ubuntu focal main" | sudo tee /etc/apt/sources.list.d/intel-sgx.list > /dev/null
wget -O - https://download.01.org/intel-sgx/sgx_repo/ubuntu/intel-sgx-deb.key | sudo apt-key add -
sudo apt update
sudo apt install sgx-pck-id-retrieval-tool
```
Or, you can [build and install][sgx-pck-id-retrieval-tool] it yourself.

2. Retrieve your machine's FMSPC by running the following command:

```shell
echo "Please enter Intel's PCS Service API key" && read -r API_KEY && PCKIDRetrievalTool -f /tmp/pckid.csv && pckid=$(cat /tmp/pckid.csv) && ppid=$(echo "$pckid" | awk -F "," '{print $1}') && cpusvn=$(echo "$pckid" | awk -F "," '{print $3}') && pcesvn=$(echo "$pckid" | awk -F "," '{print $4}') && pceid=$(echo "$pckid" | awk -F "," '{print $2}') && curl -v "https://api.trustedservices.intel.com/sgx/certification/v4/pckcert?encrypted_ppid=${ppid}&cpusvn=${cpusvn}&pcesvn=${pcesvn}&pceid=${pceid}" -H "Ocp-Apim-Subscription-Key:${API_KEY}" 2>&1 | grep -i "SGX-FMSPC"
```

<details>
<summary>Or you can retrieve FMSPC step by step</summary>


After you have installed PCKIDRetrievalTool, You should be ready to retrieve fetch Intel's certificates!

1. Run the following command:

```
PCKIDRetrievalTool
```

If successful, it should generate a `pckid_retrieval.csv`. This is a csv string which consists of:

    1. EncryptedPPID(384 BE byte array)
    2. PCE_ID(LE 16 bit integer)
    3. CPUSVN(16 byte BE byte array)
    4. PCE ISVSVN (LE 16 bit integer)
    5. QE_ID (16 byte BE byte array)

You will need this info to retrieve your FMSPC.

2. Query Intel's API to get your machine's FMSPC

```
curl -v "https://api.trustedservices.intel.com/sgx/certification/v4/pckcert?encrypted_ppid={}&cpusvn={}&pcesvn={}&pceid={}" -H "Ocp-Apim-Subscription-Key:{YOUR_API_KEY}"
```

Replace the curly braces in the above command with the values acquired from `pckid_retrieval.csv` and `YOUR_API_KEY` with your API key from subscribing to Intel's PCS Service.

The response should look as follows:

```
< HTTP/1.1 200 OK
< Content-Length: 1777
< Content-Type: application/x-pem-file
< Request-ID: e3a32aaf6cd046c69674d5bd1f1af251
< SGX-TCBm: 0B0B0303FFFF000000000000000000000D00
< SGX-PCK-Certificate-Issuer-Chain: -----BEGIN%20CERTIFICATE----- ...
-----END%20CERTIFICATE-----%0A
< SGX-PCK-Certificate-CA-Type: platform
< SGX-FMSPC: 00606A000000  <-- The FMSPC we want!
< Date: Wed, 10 Jan 2024 02:57:40 GMT
< 
-----BEGIN CERTIFICATE-----
MIIE8zCCBJmgAwIBAgIVALz+jYjxcX+fJomAUbCJqgifIol6MAoGCCqGSM49BAMC
MHAxIjAgBgNVBAMMGUludGVsIFNHWCBQQ0sgUGxhdGZvcm0gQ0ExGjAYBgNVBAoM
EUludGVsIENvcnBvcmF0aW9uMRQwEgYDVQQHDAtTYW50YSBDbGFyYTELMAkGA1UE
CAwCQ0ExCzAJBgNVBAYTAlVTMB4XDTI0MDExMDAyNDI0MFoXDTMxMDExMDAyNDI0
MFowcDEiMCAGA1UEAwwZSW50ZWwgU0dYIFBDSyBDZXJ0aWZpY2F0ZTEaMBgGA1UE
CgwRSW50ZWwgQ29ycG9yYXRpb24xFDASBgNVBAcMC1NhbnRhIENsYXJhMQswCQYD
VQQIDAJDQTELMAkGA1UEBhMCVVMwWTATBgcqhkjOPQIBBggqhkjOPQMBBwNCAAQy
sASrS6rkej14Hf1JSpuPO1NDUVyzXBCvp1h42F10UU0AFUWg1Y48oeBg7tvN5X2I
TGEB5zHBjzjv9kuWyUjUo4IDDjCCAwowHwYDVR0jBBgwFoAUlW9dzb0b4elAScnU
9DPOAVcL3lQwawYDVR0fBGQwYjBgoF6gXIZaaHR0cHM6Ly9hcGkudHJ1c3RlZHNl
cnZpY2VzLmludGVsLmNvbS9zZ3gvY2VydGlmaWNhdGlvbi92NC9wY2tjcmw/Y2E9
cGxhdGZvcm0mZW5jb2Rpbmc9ZGVyMB0GA1UdDgQWBBRTV6Zlz1vJkYSfkJj8Nifz
qgawWDAOBgNVHQ8BAf8EBAMCBsAwDAYDVR0TAQH/BAIwADCCAjsGCSqGSIb4TQEN
AQSCAiwwggIoMB4GCiqGSIb4TQENAQEEEP5GrgEczopNboM0sI0btAEwggFlBgoq
hkiG+E0BDQECMIIBVTAQBgsqhkiG+E0BDQECAQIBCzAQBgsqhkiG+E0BDQECAgIB
CzAQBgsqhkiG+E0BDQECAwIBAzAQBgsqhkiG+E0BDQECBAIBAzARBgsqhkiG+E0B
DQECBQICAP8wEQYLKoZIhvhNAQ0BAgYCAgD/MBAGCyqGSIb4TQENAQIHAgEAMBAG
CyqGSIb4TQENAQIIAgEAMBAGCyqGSIb4TQENAQIJAgEAMBAGCyqGSIb4TQENAQIK
AgEAMBAGCyqGSIb4TQENAQILAgEAMBAGCyqGSIb4TQENAQIMAgEAMBAGCyqGSIb4
TQENAQINAgEAMBAGCyqGSIb4TQENAQIOAgEAMBAGCyqGSIb4TQENAQIPAgEAMBAG
CyqGSIb4TQENAQIQAgEAMBAGCyqGSIb4TQENAQIRAgENMB8GCyqGSIb4TQENAQIS
BBALCwMD//8AAAAAAAAAAAAAMBAGCiqGSIb4TQENAQMEAgAAMBQGCiqGSIb4TQEN
AQQEBgBgagAAADAPBgoqhkiG+E0BDQEFCgEBMB4GCiqGSIb4TQENAQYEEEWJzOvy
ZE8K3kj/HhXEa/swRAYKKoZIhvhNAQ0BBzA2MBAGCyqGSIb4TQENAQcBAQH/MBAG
CyqGSIb4TQENAQcCAQH/MBAGCyqGSIb4TQENAQcDAQH/MAoGCCqGSM49BAMCA0gA
MEUCIE5VvyXrsalV8fp3Z0AbFWF4cfOJOSAaoJQLIji1TRLbAiEAsZwZGnme5EQr
n7qROhU4OOJnVs9lqNxxi8AFrJJHU2E=
-----END CERTIFICATE-----
```

</details>

Currently Supported FMSPCs (on Mainnet):
- 00606A000000
- 00A067110000
- 00906ED50000

Currently Supported FMSPCs (on Hekla):
- 00606A000000
- 00A067110000
- 00906ED50000
- 30606A000000
- 00706A100000

Please reach out to us in [discord](https://discord.com/invite/taikoxyz) channels or create an issue on Github if your machine doesn't have a listed FMSPC, if you've done the bootstrap process and obtained a quote we can try adding them to the On Chain RA process. We can't guarantee all FMSPCs will work, so you might have to switch machines. **Please include your FMSPC, CPU and your machine's EPC Size in the Github issue! This helps us decide whether the machine/FMSPC is a suitable candidate to add.**

> **_NOTE:_** At the moment, we are aware of three cloud providers who offer compatible SGX machines: [*Tencent Cloud*](https://www.tencentcloud.com/document/product/213/45510), Alibaba Cloud and Azure. (Tencent Cloud is one of our ecosystem partners!) Specifically, Tencent Cloud's `M6ce` model, Alibaba Cloud's `g7t` model support `SGX-FMSPC 00606A000000` and Azure's `confidential compute` machines support `SGX-FMSPC 00906ED50000`.
>
> If you'd like to use Tencent Cloud, they have reserved compatible `M6ce` machines available for Taiko's community with a limited time special offer. Please register [here](https://resale.anchnet.com) to purchase `M6ce` machines with discount.

[sgx-pck-id-retrieval-tool]: https://github.com/intel/SGXDataCenterAttestationPrimitives/tree/main/tools/PCKRetrievalTool

### Git

You will need to clone our `raiko` repository and `taiko-mono` repository to run Raiko and perform on-chain attestation to begin proving. An easy way to do this is with [git](https://git-scm.com/download/linux).

### Docker

You will need `docker` CLI installed, please find your respective distribution [here](https://docs.docker.com/engine/install/) and follow the install guide.

### Gramine

Raiko leverages [Intel SGX][sgx] via [Gramine][gramine]. As Gramine only supports [a limited number of distributions][gramine-distros], including Ubuntu. The Docker image is derived from Gramine's base image, which uses Ubuntu. Install your respective distribution [here](https://gramine.readthedocs.io/en/latest/installation.html).

[gramine-distros]: https://github.com/gramineproject/gramine/discussions/1555#discussioncomment-7016800
[gramine]: https://gramineproject.io/

## Raiko Docker

Once you have satisfied all the prerequisites, you can follow this section.

1. Prepare your system with some necessary installations

```
sudo apt-get update && sudo apt-get install -y build-essential wget python-is-python3 debhelper zip libcurl4-openssl-dev pkgconf libboost-dev libboost-system-dev libboost-thread-dev protobuf-c-compiler libprotobuf-c-dev protobuf-compiler
```

2. Generating PCCS Certificates

Before running the Raiko Docker container, you need to fulfill some SGX-specific prerequisites, which include setting up the [PCCS][pccs-readme] (Provisioning Certificate Caching Service) configuration. The PCCS service is responsible for retrieving PCK Certificates and other collaterals on-demand from the internet at runtime, and then caching them in a local database. The PCCS exposes similar HTTPS interfaces as Intel's Provisioning Certificate Service.

Begin the configuration process by [generating][pccs-cert-gen] an SSL certificate:

```
mkdir ~/.config
mkdir ~/.config/sgx-pccs
cd ~/.config/sgx-pccs
openssl genrsa -out private.pem 2048
chmod 644 private.pem  # Docker container needs access
openssl req -new -key private.pem -out csr.pem
openssl x509 -req -days 365 -in csr.pem -signkey private.pem -out file.crt
rm csr.pem
```

[pccs-readme]: https://github.com/intel/SGXDataCenterAttestationPrimitives/blob/master/QuoteGeneration/pccs/README.md
[pccs-cert-gen]: https://github.com/intel/SGXDataCenterAttestationPrimitives/tree/master/QuoteGeneration/pccs/container#2-generate-certificates-to-use-with-pccs

3. Curl the config file

```
curl -s https://raw.githubusercontent.com/taikoxyz/raiko/refs/heads/main/docs/default.json > ~/.config/sgx-pccs/default.json
```

Make sure you've copied the `default.json` into the .config/sgx-pccs directory you created earlier. The `raiko` container will mount this as a volume. After copying the file, open it for editing and fill in the below listed parameters as recommended by [Intel's manual][pccs-cert-gen-config]:

- `ApiKey`: The PCCS uses this API key to request collaterals from Intel's Provisioning Certificate Service. User needs to subscribe first to obtain an API key. Use either the primary or secondary key you obtained from the previous step `Subscribing to Intel PCS Service`.

- `UserTokenHash`: SHA512 hash of the user token for the PCCS client user to register a platform. For example, PCK Cert ID retrieval tool will use the user token to send platform information to PCCS. (`echo -n "user_password" | sha512sum | tr -d '[:space:]-'`).

- `AdminTokenHash`: SHA512 hash of the administrator token for the PCCS administrator to perform a manual refresh of cached artifacts (`echo -n "admin_password" | sha512sum | tr -d '[:space:]-'`).

- `hosts`: replace it with "0.0.0.0".

Ensure docker can use it by modifying permissions to the file:
 
```
chmod 644 default.json
```

[pccs-cert-gen-config]: https://github.com/intel/SGXDataCenterAttestationPrimitives/tree/master/QuoteGeneration/pccs/container#3-fill-up-configuration-file

4. Make some directories to prevent errors

```
mkdir ~/.config/raiko
mkdir ~/.config/raiko/config
mkdir ~/.config/raiko/secrets
```

5. Now, clone raiko and check out the respective branch and navigate to the `docker` folder. From here you can pull the images from our registry.

For Taiko Alethia, the branch should be `v1.8.0`; for Taiko Hekla, the branch should be `v1.9.0-rc.1`.

```
git clone https://github.com/taikoxyz/raiko.git
cd raiko/docker
git checkout tags/{BRANCH_TAG}
```

> **_NOTE:_** For Taiko Alethia: You will need to modify your raiko `docker-compose.yml` to use the images you pull. If you are using a SGX2 machine, please use `1.8.0-edmm`. If you are using a SGX1 machine, please use `1.8.0`.

> **_NOTE:_** For Taiko Hekla: You will need to modify your raiko `docker-compose.yml` to use the images you pull. If you are using a SGX2 machine, please use `1.9.0-rc.1-edmm`. If you are using a SGX1 machine, please use `1.9.0-rc.1`.

In your `docker-compose.yml` file, search for `raiko:latest` and change all instances to `raiko:{TAG}`. Use the following commands to pull the respective images.

```
docker pull us-docker.pkg.dev/evmchain/images/raiko:{TAG}
docker pull us-docker.pkg.dev/evmchain/images/pccs:latest
```

It is not recommended, but you can also build the raiko image locally with `docker compose build raiko`. If you do so, you do not need to change `raiko:latest` in your docker-compose.yml. You may run into issues if the versions do not line up with the verifier, so we highly recommend using the registry images.

6. Check that the images have been pulled

```
docker image ls
```

You should see at least two images, `us-docker.pkg.dev/evmchain/raiko` and `us-docker.pkg.dev/evmchain/pccs`.

7. Create a `.env` using the `.env.sample` template. 

You can copy the template with the following:

```
cp .env.sample .env
```

If you are running Raiko for Taiko Hekla, ensure `SGXGETH=true`, `NETWORK=taiko_a7` and `L1_NETWORK=holesky` in `.env`.
If you are running Raiko for Taiko Alethia, ensure `SGXGETH=true`, `NETWORK=taiko_mainnet` and `L1_NETWORK=ethereum` in `.env`.

8. Bootstrap Raiko with the following command:

```
docker compose up init
```

If everything is configured correctly, raiko-init should run without errors and generate a `bootstrap.json` and `bootstrap.gaiko.json`. Check that they exist with the following command:

```
ls ~/.config/raiko/config
```

You've now prepared your machine for running Raiko through Docker. Now, you need to perform On-Chain Remote Attestation to receive TTKOh from moderators and begin proving for Taiko!

## On-Chain RA

1. Clone [taiko-mono](https://github.com/taikoxyz/taiko-mono/tree/main) and navigate to the protocol directory.

```
cd ~
git clone https://github.com/taikoxyz/taiko-mono.git
cd taiko-mono
cd packages/protocol
```

2. Install [`pnpm`](https://pnpm.io/installation#on-posix-systems) and [`foundry`](https://book.getfoundry.sh/getting-started/installation) so that you can install dependencies for taiko-mono.

```
curl -fsSL https://get.pnpm.io/install.sh | sh -
curl -L https://foundry.paradigm.xyz | bash
source ~/.bashrc
foundryup
```

Once you have installed them, run the following:

```
pnpm install
pnpm compile
```

3. If you've followed the Raiko Docker guide, you will have bootstrapped raiko and obtained two bootstrap.json files. (`bootstrap.json` and `bootstrap.gaiko.json`) Both will look like the following:

```
"public_key": "0x02ab85f14dcdc93832f4bb9b40ad908a5becb840d36f64d21645550ba4a2b28892",
"new_instance": "0xc369eedf4c69cacceda551390576ead2383e6f9e",
"quote": "0x030002......f00939a7233f79c4ca......9434154452d2d2d2d2d0a00"
```

You can find it with `cat ~/.config/raiko/config/bootstrap.json` or  `cat ~/.config/raiko/config/bootstrap.gaiko.json`.

4. Export an RPC url for the L1 network you are registering for. i.e. `FORK_URL=https://any_holesky_rpc_url/` for Hekla, `FORK_URL=https://any_ethereum_rpc_url/` for Alethia.

5. Call the script with `PRIVATE_KEY=0x{YOUR_PRIVATE_KEY} ./script/layer1/provers/config_dcap_sgx_verifier.sh --env {NETWORK} --quote {YOUR_QUOTE_HERE}`. "YOUR_QUOTE_HERE" comes from above step 5.

`NETWORK` will be `hekla-pacaya-<sgxreth|sgxgeth>` or `mainnet-pacaya-<sgxreth|sgxgeth>` depending on which verifier you are registering to.

 You will have to do this step twice: once for SgxGeth and once for Pacaya. Please use the quote in `bootstrap.gaiko.json` to register for `<mainnet|hekla>-pacaya-sgxgeth` and the quote from `bootstrap.json` to register for `<mainnet|hekla>-pacaya-sgxreth`. Keep both instance IDs.

6. If you've been successful, you will get a SGX instance `id` which can be used to run Raiko!

It should look like this:

```
emit InstanceAdded(id: 1, instance: 0xc369eedf4C69CacceDa551390576EAd2383E6f9E, replaced: 0x0000000000000000000000000000000000000000, validSince: 1708704201 [1.708e9])
```

If you accidentally cleared your terminal or somehow otherwise fail to view this event log, you can find this value in the Etherscan at your prover EOA.
You should see a new transaction with the method `Register Instance` sent to the respective `SGX_VERIFIER_ADDRESS`; viewing the transaction details and accessing the transaction receipt event logs should show the InstanceAdded event.

## Running Raiko

Once you've completed the above steps, you can actually run a prover. 

Raiko now supports more configurations, which need to be carefully checked to avoid errors. These can be found in the `raiko/docker/.env.sample`, which you should have copied and be using as your `.env`.

    - SGX_ONTAKE_INSTANCE_ID: SGX registered ID for ontake fork. (DEPRECATED)
    - SGX_PACAYA_INSTANCE_ID: SGX registered ID for pacaya fork. (if raiko is started in pacaya, set this one)
    - SGXGETH_PACAYA_INSTANCE_ID： registered instance ID for the sgxgeth proof for pacaya fork. (must be set for pacaya with sgxgeth)
    - ETHEREUM_RPC: ethereum node url, from which you query the ethereum data.
    - ETHEREUM_BEACON_RPC: ethereum beacon node url, from which you query the ethereum data.
    - HOLESKY_RPC: ethereum holesky test node url.
    - HOLESKY_BEACON_RPC: ethereum holesky test beacon node url.
    - TAIKO_A7_RPC: taiko hekla(a7) testnet node url.
    - TAIKO_MAINNET_RPC: taiko mainnet node url.
    - L1_NETWORK: specify the l1 network if exist, default is "holesky".
    - NETWORK: specify the network to be proven, could be one of ["taiko_a7", "taiko_mainnet", "ethereum", "holesky"], default is "taiko_a7". make sure both L1_NETWORK & NETWORK in chain_spec_list.docker.json

A most common setup in hekla testnet when as of the Pacaya fork is:
```
cd ~/raiko/docker
export SGX_PACAYA_INSTANCE_ID={YOUR_PACAYA_INSTANCE_ID}
export SGXGETH_PACAYA_INSTANCE_ID={YOUR_SGXGETH_INSTANCE_ID}
export L1_NETWORK="holesky"
export NETWORK="taiko_a7"
export HOLESKY_RPC={YOUR_FAST_HOLESKY_NODE}
export HOLESKY_BEACON_RPC={YOUR_FAST_HOLESKY_BEACON_NODE}
export TAIKO_A7_RPC={YOUR_FAST_A7_NODE}
docker compose up raiko -d
```

You can alternatively set all these values in the `.env` file in `raiko/docker`.

If everything is working, you should see something like the following when executing `docker compose logs raiko`:

```
Start config:
Object {
    "address": String("0.0.0.0:8080"),
    "cache_path": Null,
    "concurrency_limit": Number(16),
    "config_path": String("/etc/raiko/config.sgx.json"),
    "log_level": String("info"),
    "log_path": Null,
    "max_log": Number(7),
    "network": String("taiko_a7"),
    "sgx": Object {
        "instance_id": Number(19), <--- sgx instance id
    },
}
Args:
Opt {
    address: "0.0.0.0:8080",
    concurrency_limit: 16,
    log_path: None,
    max_log: 7,
    config_path: "/etc/raiko/config.sgx.json",
    cache_path: None,
    log_level: "info",
}

...

raiko  | + jq --arg update_value X '.sgx.instance_ids.PACAYA = ($update_value | tonumber)' /etc/raiko/config.sgx.json
raiko  | + mv /tmp/config_tmp.json /etc/raiko/config.sgx.json
raiko  | + echo 'Update pacaya sgx instance id to X'
raiko  | Update pacaya sgx instance id to X
raiko  | + [[ -n Y ]]
raiko  | + jq --arg update_value Y '.sgxgeth.instance_ids.PACAYA = ($update_value | tonumber)' /etc/raiko/config.sgx.json
raiko  | + mv /tmp/config_tmp.json /etc/raiko/config.sgx.json
raiko  | + echo 'Update pacaya sgxgeth instance id to Y'
raiko  | Update pacaya sgxgeth instance id to Y

...

2024-04-18T12:50:09.400319Z  INFO raiko_host::server: Listening on http://0.0.0.0:8080
```

## Upgrading Raiko (Mainnet)

If you previously ran an instance of Raiko and are looking to upgrade it, this section covers the only necessary steps.

1. Take down your previous Raiko instance

Navigate to `raiko/docker` and run the following command:

`docker compose down raiko -v`

Remove your previously autogenerated priv.key:

`sudo rm ~/.config/raiko/secrets/priv.key`

2. Checkout the relevant tag/branch

`git checkout tags/v1.8.0` for the upcoming Mainnet upgrade.

3. Copy the sample `.env` and make the following changes:

```bash
cp .env.sample .env
vi .env
- SGXGETH=true
- NETWORK=taiko_mainnet
- L1_NETWORK=ethereum
```

4. Pull the image from our registry or build the image locally:

Pull the image from the registry, use `docker compose pull us-docker.pkg.dev/evmchain/images/raiko:{TAG}`

`TAG` should be `1.8.0` if you are using SGX1, `1.8.0-edmm` if you are using SGX2.

If you prefer, you can build the image with the following command: `docker compose build raiko`.

5. Modify your docker-compose.yml file to use the image.

`vi docker-compose.yml`

set all instances of raiko image to raiko:1.8.0 or raiko:1.8.0-edmm

6. Bootstrap your instance

`docker compose up init`

If there are no errors, please use `ls ~/.config/raiko/config` to check that the files `bootstrap.json` and `bootstrap.gaiko.json` exist.

7. Navigate to `taiko-mono` and register your instance.

If you haven't done so yet, clone `taiko-mono`. Checkout `main`.

```bash
cd packages/protocol
export PRIVATE_KEY=0x{YOUR_PRIVATE_KEY}
export FORK_URL={ETH_RPC_URL}
./script/layer1/provers/config_dcap_sgx_verifier.sh --env mainnet-pacaya-sgxreth --quote {QUOTE_FROM_BOOTSTRAP.JSON}
./script/layer1/provers/config_dcap_sgx_verifier.sh --env mainnet-pacaya-sgxgeth --quote {QUOTE_FROM_BOOTSTRAP.GAIKO.JSON}
```
You will use the instance ids for the next step.

8. Navigate back to Raiko and modify .env again.

```bash
cd raiko/docker
vi .env
```

Set `SGX_PACAYA_INSTANCE_ID` to the instance id from the sgxreth run, and `SGXGETH_PACAYA_INSTANCE_ID` to the instance id from the sgxgeth run.

9. Start your Raiko instance again

`docker compose up raiko -d`. You can verify if it's running properly with the tests described in the guide below.

## Verify that your Raiko instance is running properly (Pacaya and SgxGeth)

As of the Pacaya fork (currently only in Hekla), you will need to check that your Raiko instance can prove batches.

Please make sure that you have done the On Chain RA step with the Pacaya addresses and exported the your `SGX_PACAYA_INSTANCE_ID` before running Raiko. The same must be done for the SgxGeth proof, and `SGXGETH_PACAYA_INSTANCE_ID` must be set too.

Use `./script/prove-batch.sh taiko_a7 sgx "[(1407735,3881175)]"` to check readiness. 

> **_NOTE:_** If you would like to check the sgxgeth is set up properly, simply replace `sgx` with `sgxgeth`. The responses should look the same, except with `proof_type: sgxgeth`. For the curl response, check the script for the `proofParams` to replace.

The initial response will be as follows:
```
Parsed batch request: [{"batch_id": 1407735, "l1_inclusion_block_number": 3881175}]
{"data":{"status":"registered"},"proof_type":"sgx","status":"ok"}
```

You may then navigate to `raiko/docker` and check the logs with `docker compose logs raiko`. If you see the following log, your prover is functional and working as intended!

```
raiko  | 2025-03-31T22:41:16.762651Z  INFO raiko_reqpool::pool: RedisPool.update_status: {"BatchProof":{"chain_id":167009,"batch_id":1407735,"l1_inclusion_height":3881175,"proof_type":"sgx","prover_address":"0x70997970C51812dc3A010C7d01b50e0d17dc79C8"}}, Success
raiko  | 2025-03-31T22:41:16.762696Z  INFO raiko_reqpool::pool: RedisPool.add: {"BatchProof":{"chain_id":167009,"batch_id":1407735,"l1_inclusion_height":3881175,"proof_type":"sgx","prover_address":"0x70997970C51812dc3A010C7d01b50e0d17dc79C8"}}, Success
```

Alternatively, you may wait a minute or so and call `./script/prove-batch.sh taiko_a7 sgx "[(1407735,3881175)]"` again: this time if the response is as follows:

```
Parsed batch request: [{"batch_id": 1407735, "l1_inclusion_block_number": 3881175}]
{"data":{"proof":{"input":"0x779c2bc712311b754f7a71fd2065f337fbabd7473b4b231164ea1a51e39816d9","kzg_proof":null,"proof":0x0000...,"quote":03002...,"uuid":null}},"proof_type":"sgx","status":"ok"}
```

Your Raiko instance is correctly configured and working for the Pacaya fork.

If you would like to use a curl request instead, try the following:

```
curl --location --request POST 'http://localhost:8080/v3/proof/batch' \
    --header 'Content-Type: application/json' \
    --header 'Authorization: Bearer' \
    --data-raw '{
        "network": "taiko_a7",
        "l1_network": "holesky",
        "batches": [{"batch_id": 1407735, "l1_inclusion_block_number": 3881175}],
        "prover": "0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
        "graffiti": "0x0000000000000000000000000000000000000000000000000000000000000000",
        "proof_type": "sgx",
        "blob_proof_type": "proof_of_equivalence",
        "proof_type": "sgx",
        "sgx" : {
            "instance_id": 123,
            "setup": false,
            "bootstrap": false,
            "prove": true,
            "input_path": null
        }
    }'
```

The responses should be the same as listed above.

If you would like to test multiple batches, you can add them like so: `./script/prove-batch.sh taiko_a7 sgx "[(1407735,3881175), ($batch_id2,$batch_height2)]"`
You will only see logs for one of the batches in the format as above but you can check `docker compose logs raiko` to see if both batches were proved successfully.

## Verify that your Raiko instance can successfully aggregation prove

Now that we offer aggregation proving, it may be useful to test if the functionality is as you expect. Run the following script:

`./script/prove-aggregation-blocks.sh taiko_mainnet native 800000`. You may switch `native` with `sgx` to be doubly sure that the sgx proof generation is functional.

This will test the batch proving on block 799999 and 800000. If you see the log `Aggregate proof successful.` then it is functioning normally! 

If you use blocks that are too old, it may hang and fail; please try to use more recent blocks.

