# Raiko Docker Setup Tutorial

This tutorial was created to assist you in setting up Raiko and its SGX dependencies using a Docker container. Configuring SGX can be complex without a detailed guide to walk you through each step. This tutorial strives to provide a comprehensive walkthrough, leaving no detail unaddressed.

## Prerequisites

Raiko leverages [Intel SGX][sgx] via [Gramine][gramine]. As Gramine only supports [a limited number of distributions][gramine-distros], including Ubuntu. The Docker image is derived from Gramine's base image, which uses Ubuntu.

Intel SGX is a technology that involves a considerable amount of configuration. Given its high level of configurability, the setup of your infrastructure may vary significantly depending on the attestation type (EPID, ECDSA) and other parameters. While we've strived to minimize the manual effort required to prepare the development environment, there are certain prerequisites that are challenging, if not impossible, to automate using Dockerfiles. This section outlines these prerequisites.

[gramine-distros]: https://github.com/gramineproject/gramine/discussions/1555#discussioncomment-7016800
[gramine]: https://gramineproject.io/

### Intel SGX-enabled CPU

Ensure that your machine has an [Intel SGX][sgx]-enabled CPU to run Raiko. You can verify if your CPU supports SGX (Software Guard Extensions) on Linux by using the [`cpuid`][cpuid] tool.

1. If `cpuid` isn't already installed, you can install it. On Ubuntu, use the following command:

        sudo apt-get install cpuid

1. Run `cpuid` and `grep` for `sgx`:

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

[kernel-5.11]: https://www.intel.com/content/www/us/en/developer/tools/software-guard-extensions/linux-overview.html
[edmm]: https://gramine.readthedocs.io/en/stable/manifest-syntax.html#edmm

### Generating PCCS Certificates

Before running the Raiko Docker container, you need to fulfill some SGX-specific prerequisites, which include setting up the [PCCS][pccs-readme] (Provisioning Certificate Caching Service) configuration. The PCCS service is responsible for retrieving PCK Certificates and other collaterals on-demand from the internet at runtime, and then caching them in a local database. The PCCS exposes similar HTTPS interfaces as Intel's Provisioning Certificate Service.

Begin the configuration process by [generating][pccs-cert-gen] an SSL certificate:

```
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

### Subscribing to Intel PCS Service

To use ECDSA Attestation, you need to subscribe to the Intel PCS service, following the steps in [Intel's how-to guide][intel-dcap-install-howto]. After subscribing to the service, you will get two keys: a primary API key and a secondary API key.

To use these keys in your configuration file, copy the `config/default.json` [template config file][pccs-config-file] to `$HOME/.config/sgx-pccs`. The `raiko` container will mount this as a volume. After copying the file, open it for editing and fill in the below listed parameters as recommended by [Intel's manual][pccs-cert-gen-config]:
- `ApiKey`: The PCCS uses this API key to request collaterals from Intel's Provisioning Certificate Service. User needs to subscribe first to obtain an API key. For how to subscribe to Intel Provisioning Certificate Service and receive an API key, goto https://api.portal.trustedservices.intel.com/provisioning-certification and click on `Subscribe`.

- `UserTokenHash`: SHA512 hash of the user token for the PCCS client user to register a platform. For example, PCK Cert ID retrieval tool will use the user token to send platform information to PCCS. (`echo -n "user_password" | sha512sum | tr -d '[:space:]-'`).

- `AdminTokenHash`: SHA512 hash of the administrator token for the PCCS administrator to perform a manual refresh of cached artifacts (`echo -n "admin_password" | sha512sum | tr -d '[:space:]-'`).

> **_NOTE:_** These tokens can be set to whatever you like, the above commands set them to `"user_password" and "admin_password"`.

[intel-dcap-install-howto]: https://www.intel.com/content/www/us/en/developer/articles/guide/intel-software-guard-extensions-data-center-attestation-primitives-quick-install-guide.html
[pccs-cert-gen-config]: https://github.com/intel/SGXDataCenterAttestationPrimitives/tree/master/QuoteGeneration/pccs/container#3-fill-up-configuration-file
[pccs-config-file]: https://github.com/intel/SGXDataCenterAttestationPrimitives/blob/main/QuoteGeneration/pccs/config/default.json

## Building Docker image

Taiko doesn't currently offer a prebuilt Docker image. You will need to build it yourself using the `docker-compose` file we provide. Two Docker images need to be built: `raiko` and the SGX-specific `pccs` service, which manages the lifecycle of the certificates required for [ECDSA attestation][ecdsa].

1. Clone `raiko` repository:
   ```
   git clone https://github.com/taikoxyz/raiko.git
   ```
1. Checkout the `taiko/alpha-7` branch.
   ```
   cd raiko
   git checkout taiko/alpha-7
   ```
1. Change active directory:
   ```
   cd docker
   ```
1. Build the image:
   ```
   docker compose build
   ```
> **_NOTE:_** If you're running into issues where Docker's `$HOME` var is set strangely, it might have to do with your docker installation; try reinstalling with [this][docker] guide! 

1. That's it! You should now see two Raiko images, `raiko` and `pccs`, in your Docker images list. You can view this list by running the following command:
   ```
   docker image ls
   ```

[ecdsa]: https://github.com/cloud-security-research/sgx-ra-tls/blob/master/README-ECDSA.md
[docker]: https://docs.docker.com/engine/install/ubuntu/

## Running PCCS service

Before starting Raiko, the PCCS service must be up and running. To run the PCCS service, you must configure it with the SSL certificate that you generated in a previous section of this document:

```
docker compose up pccs -d
```

Verify the successful start of the PCCS service by executing:

```
docker logs pccs
```

If everything is set up correctly, you should see:

```
HTTPS Server is running on: https://localhost:8081
```

You can now bootstrap and run Raiko as a daemon.

## Retrieving PCK Certs

Now, we need to retrieve Intel's PCK Certificates and populate the PCCS service with them.

Install the `PCKIDRetrievalTool`

You can install either from the Ubuntu repository:
```
$ echo ’deb [arch=amd64] https://download.01.org/intel-sgx/sgx_repo/ubuntu
focal main’ | sudo tee /etc/apt/sources.list.d/intel-sgx.list > /dev/null
$ wget -O - https://download.01.org/intel-sgx/sgx_repo/ubuntu/intel-sgx-deb.key | sudo apt-key add -
$ sudo apt update
$ sudo apt install sgx-pck-id-retrieval-tool
```
Or, you can [build and install][sgx-pck-id-retrieval-tool] it yourself. 

After you have installed it, You should be ready to retrieve fetch Intel's certificates!

To do this, open the file `/opt/intel/sgx-pck-id-retrieval-tool/network_setting.conf` and add the following lines:

```
PCCS_URL=https://localhost:8082/sgx/certification/v3/platforms
user_token=<USER_TOKEN>
USE_SECURE_CERT=FALSE
```

Replace `<USER_TOKEN>` with the user password you got when you subscribed to the Intel PCS Service, as described in a previous steps of this tutorial.

Then you can run the following command:

```
PCKIDRetrievalTool
```

Alternatively, you can skip editing the `network_setting.conf` configuration file and directly run the command with flags
```
PCKIDRetrievalTool -url https://localhost:8082/sgx/certification/v3/platforms -user_token '<USER_TOKEN>' -use_secure_cert false
```

If everything was successful, you should receive a non-empty response when you make the following request:

```
curl -k -G "https://localhost:8081/sgx/certification/v3/rootcacrl"
```

Now, you're ready to bootstrap and run Raiko!

[sgx-pck-id-retrieval-tool]: https://github.com/intel/SGXDataCenterAttestationPrimitives/tree/main/tools/PCKRetrievalTool

### Raiko bootstrapping

Bootstrapping involves generating a public-private key pair, which is used for signatures within the SGX enclave. The private key is stored in an [encrypted][gramine-encrypted-files] format in the `~/.config/raiko/secrets/priv.key` file. The encryption and decryption processes occur inside the enclave, offering protection against malicious attacks.

1. Make sure you haven't generated Raiko's public-private key pair yet:
   ```
   ls ~/.config/raiko/secrets
   ```
   If you `secrets` directory isn't empty, you can skip Raiko bootstrapping.
1. Bootstrap Raiko:
   ```
   docker compose run --rm raiko --init
   ```
   It creates a new, encrypted private key in `~/.config/raiko/secrets` directory and couple more configuration files under `$HOME/.config/raiko` directory:
   ```
   $ tree ~/.config/raiko
   /home/ubuntu/.config/raiko
   ├── config
   │   ├── bootstrap.json
   │   ├── raiko-guest.manifest.sgx
   │   └── raiko-guest.sig
   └── secrets
      └── priv.key
   ```
   You can inspect your public key, instance ID, and SGX quote in the file `$HOME/.config/raiko/bootstrap.json`.

[gramine-encrypted-files]: https://gramine.readthedocs.io/en/stable/manifest-syntax.html#encrypted-files

### Running Raiko daemon

Once you have Raiko bootstrapped, you can start Raiko daemon.

```
docker compose up raiko -d
```

Start the Raiko daemon. Skip `-d` (which stands for _daemon_) to run in the foreground instead.

### Test Raiko

Now, once you have Raiko up and running, you can test it to make sure it is serving requests as expected.

1. Open new terminal and run:
   ```
   tail -f /var/log/raiko/raiko.log.dd-mm-yyyy
   ```
   to monitor requests that you will be sending. Replace `dd-mm-yyyy` placeholder with the current date.
1. Send a sample request to Raiko:
   ```
   curl --location --request POST 'http://localhost:8080' --header 'Content-Type: application/json' --data-raw '{
      "jsonrpc": "2.0",
      "method": "proof",
      "params": [
         {
               "type": "Sgx",
               "block": 3000,
               "l2Rpc": "https://rpc.internal.taiko.xyz/",
               "l1Rpc": "https://l1rpc.internal.taiko.xyz/",
               "l1BeaconRpc": "https://l1beacon.internal.taiko.xyz/",
               "prover": "0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
               "graffiti": "0x6162630000000000000000000000000000000000000000000000000000000000"
         }
      ],
      "id": 0
   }'
   ```
   If the request was served correctly, you should see a lot of logs being produced in the log file and an SGX proof printed on the standard output:
   ```
   {"jsonrpc":"2.0","id":1,"result":{"type":"Sgx","proof":"0x000000006cbe8f8cb4c319f5beba9a4fa66923105dc90aec3c5214eed022323b9200097b647208956cc1b7ce0d8c0777df657caace329cc73f2398b137095128c7717167fc52d6474887e98e0f97149c9be2ca63a458dc8a1b"}}
   ```
