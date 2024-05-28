# Raiko Docker upgrade Tutorial

This is a quick guide for those who want to upgrade their simple taiko node and Raiko to the latest release (v1.0.0). For a comprehensive guide, please check the main guide. 

## For Taiko node:

```
cd simple-taiko-node
sudo docker compose down  taiko_client_proposer taiko_client_driver l2_execution_engine taiko_client_prover_relayer prometheus grafana
```

Update the repository:
```git pull```

Change the branch to the new release:
```
git checkout hekla
```

Start the node to sync:
```
docker compose --profile l2_execution_engine up -d
```

## For Raiko:

Remove priv.key and bootstrap.json files first:
```
cd ~/.config/raiko/secrets
rm -r priv.key
```

```
cd ~/.config/raiko/config
rm -r bootstrap.json
```

Clone the new release:
```
rm -r ~/raiko
git clone --branch release/v1.0.0 https://github.com/taikoxyz/raiko.git
cd raiko/docker
docker compose build
```

This will take around 5 minutes (same as before).
Once completed:
```
docker compose up init
```

Register the SGX ID:
```
cd ~/taiko-mono/packages/protocol
sudo apt install jq -y
export FORK_URL=replace_with_your_holesky_rpc
QUOTE=$(cat ~/.config/raiko/config/bootstrap.json | jq -r '.quote')
PRIVATE_KEY=0xyour_prover_private_key ./script/config_dcap_sgx_verifier.sh --quote $QUOTE
```

Then export SGX ID and run Raiko:
```
cd ~/raiko/docker
export SGX_INSTANCE_ID={YOUR_INSTANCE_ID}
export HOLESKY_RPC={YOUR_FAST_HOLESKY_NODE}
export HOLESKY_BEACON_RPC={YOUR_FAST_HOLESKY_BEACON_NODE}
docker compose up raiko -d
```

Now go back to Taiko node, check Grafana to make sure it's synced. Then stop and start it again. 
```
cd simple-taiko-node
sudo docker compose down  taiko_client_proposer taiko_client_driver l2_execution_engine taiko_client_prover_relayer prometheus grafana
sudo docker compose up  taiko_client_proposer taiko_client_driver l2_execution_engine taiko_client_prover_relayer prometheus grafana -d

```

Checke prover logs:
sudo docker compose logs -f taiko_client_prover_relayer

If you get this error;
"flag provided but not defined: -raiko.l2
Incorrect Usage: flag provided but not defined: -raiko.l2"
You need to remove the flag from start-prover-relayer.sh :
```
cd ~/simple-taiko-node/script
nano start-prover-relayer.sh
```
and
Remove this line: 
else
        ARGS="${ARGS} --raiko.l2 ${TAIKO_NODE_IP}:${PORT_L2_EXECUTION_ENGINE_HTTP}"

Save the file and go back to simple-taiko-node directory, and stop and start the prover. 

Check Prover logs again.