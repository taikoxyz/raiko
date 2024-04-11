# On-chain RA Tutorial

This tutorial was created to assist you in verifying your SGX machine for proving and obtaining TTKOh to begin the proving process. We have an automated process for delivering TTKOh to all provers who participate in on-chain RA!

## Prerequisites

You must have a Raiko instance set up, following the [README_Docker](https://github.com/taikoxyz/raiko/blob/taiko/alpha-7/README_Docker.md) guide in this repo.


### Procedure
1. Clone [taiko-mono](https://github.com/taikoxyz/taiko-mono/tree/main) and navigate to the scripts

```
git clone https://github.com/taikoxyz/taiko-mono.git
cd taiko-mono/packages/protocol/script
```

2. Prepare your prover's private key and the following addresses:
```
PRIVATE_KEY={PROVER_PRIVATE_KEY} \
SGX_VERIFIER_ADDRESS=0x532EFBf6D62720D0B2a2Bb9d11066E8588cAE6D9 \
ATTESTATION_ADDRESS=0xC6cD3878Fc56F2b2BaB0769C580fc230A95e1398 \
PEM_CERTCHAIN_ADDRESS=0x08d7865e7F534d743Aba5874A9AD04bcB223a92E \
```

Replace these values in the `config_dcap_sgx_verifier.sh` script.

3. If you've followed the [README_Docker](https://github.com/taikoxyz/raiko/blob/taiko/alpha-7/README_Docker.md) guide, you will have bootstrapped raiko and obtained a quote:

```
"public_key": "0x02ab85f14dcdc93832f4bb9b40ad908a5becb840d36f64d21645550ba4a2b28892",
"new_instance": "0xc369eedf4c69cacceda551390576ead2383e6f9e",
"quote": "0x030002......f00939a7233f79c4ca......9434154452d2d2d2d2d0a00"
```

Take that quote and replace `V3_QUOTE_BYTES` in the `config_dcap_sgx_verifier.sh` script.

4. Call the script with `./config_dcap_sgx_verifier.sh`.

> **_NOTE:_**  If you already have QE/TCB/Enclave already configured you can change `export TASK_ENABLE="1,1,1,1,1"` to `export TASK_ENABLE="0,0,0,0,1"` to only register the SGX instance.

5. If you've been successful, you will get a SGX instance `id` which can be used to run Raiko!

It should look like this:
```
emit InstanceAdded(id: 1, instance: 0xc369eedf4C69CacceDa551390576EAd2383E6f9E, replaced: 0x0000000000000000000000000000000000000000, validSince: 1708704201 [1.708e9])
```