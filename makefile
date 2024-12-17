
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
