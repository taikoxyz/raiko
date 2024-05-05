
ifndef DEBUG
	FLAGS = --release
endif

install:
	./scripts/install.sh $(TARGET)

risc0:
	@cargo +stable run --bin risc0-builder
	cargo +stable build $(FLAGS) --features risc0

sp1:
	@cargo +nightly-2024-02-06 run --bin sp1-builder
	cargo +nightly-2024-02-06 build $(FLAGS) --features sp1

sgx:
	cargo +stable build $(FLAGS) --features sgx
