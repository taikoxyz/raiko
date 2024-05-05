
ifdef DEBUG
	FLAGS = --release
else
	FLAGS = --debug
endif

install:
	./scripts/install.sh $(TARGET)

risc0:
	@cargo +nightly-2024-02-06 run --bin risc0-builder
	cargo +nightly-2024-02-06 build $(FLAGS) --features risc0

sp1:
	@cargo +nightly-2024-02-06 run --bin sp1-builder
	cargo +nightly-2024-02-06 build $(FLAGS) --features sp1

sgx:
	cargo +nightly-2024-02-06 build $(FLAGS) --features sgx
