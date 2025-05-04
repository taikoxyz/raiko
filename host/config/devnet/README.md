Copy env exmaple and edit the vars
cp .env.example .env

Register SGX instance
./host/config/devnet/set-dcap-params.sh path/to/your.env

Set SP1 trusted program VKs
./host/config/devnet/set-program-trusted.sh path/to/your.env

Set RISC0 trusted images
./host/config/devnet/set-image-id-trusted.sh path/to/your.env
