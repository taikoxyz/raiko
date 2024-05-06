
install:
	./scripts/install.sh $(TARGET)

build:
	./scripts/run.sh $(TARGET)

run:
	RUN=1 ./scripts/run.sh $(TARGET)

test:
	TEST=1 ./scripts/run.sh $(TARGET)

fmt:
	@cargo fmt --all --check

clippy:
	@cargo +nightly-2024-02-06 check --features "sgx,sp1,risc0"
	@cargo +nightly-2024-02-06 clippy --workspace --features "sgx,sp1,risc0" --all-targets -- -Dwarnings
