
.PHONY: help
help: ## Display this help.
	@awk 'BEGIN {FS = ":.*##"; printf "Usage:\n  make \033[36m<target>\033[0m\n"} /^[a-zA-Z_0-9-]+:.*?##/ { printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2 } /^##@/ { printf "\n\033[1m%s\033[0m\n", substr($$0, 5) } ' $(MAKEFILE_LIST)

install: ## Install the project
	./script/install.sh $(TARGET)

guest: ## Build the guest binary
	GUEST=1 ./script/build.sh $(TARGET)

build: # Build the project
	./script/build.sh $(TARGET)

run: ## Run the project
	RUN=1 ./script/build.sh $(TARGET)

test: ## Run the tests
	TEST=1 ./script/build.sh $(TARGET)
	TEST=1 RUN=1 ./script/build.sh $(TARGET)

integration: ## Run the integration tests
	CONFIG_PATH="config/config.json" ./script/integration.sh $(TARGET)

fmt: ## Run cargo fmt
	@cargo fmt --all --check

clippy: ## Run cargo clippy
	CLIPPY=1 ./script/build.sh $(TARGET)

update: ## Run cargo update
	@cargo update
	cd ./provers/sp1/guest && cargo update
	cd ./provers/risc0/guest && cargo update

##@ Raiko V2 (zkVM only)

raiko2: ## Build raiko2 binary
	cargo build --release -p raiko2

raiko2-check: ## Check all raiko2 crates
	cargo check -p raiko2-primitives -p raiko2-driver -p raiko2-provider \
		-p raiko2-stateless -p raiko2-prover -p raiko2-engine -p raiko2-protocol -p raiko2

raiko2-run: ## Run raiko2 server
	cargo run --release -p raiko2 -- $(ARGS)

raiko2-guests: ## Build raiko2 zkVM guest programs
	./script/build-guest.sh risc0
	./script/build-guest.sh sp1

raiko2-test: ## Run raiko2 tests
	cargo test -p raiko2-primitives -p raiko2-driver -p raiko2-provider \
		-p raiko2-stateless -p raiko2-prover -p raiko2-engine -p raiko2-protocol

raiko2-clippy: ## Run clippy on raiko2 crates
	cargo clippy -p raiko2-primitives -p raiko2-driver -p raiko2-provider \
		-p raiko2-stateless -p raiko2-prover -p raiko2-engine -p raiko2-protocol -p raiko2 -- -D warnings

raiko2-docker: ## Build raiko2 Docker image
	docker build -f Dockerfile.raiko2 -t raiko2:latest .

