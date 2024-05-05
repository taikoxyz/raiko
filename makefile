
ifdef DEBUG
	FLAGS = --release
else
	FLAGS = --debug
endif

install:
	./install.sh $(TARGET)

risc0:
	@./toolchain.sh risc0
	@cargo +stable run --bin risc0-builder
	cargo +stable build $(FLAGS) --features risc0

sp1:
	@./toolchain.sh sp1
	@cargo +nightly-2024-02-06 run --bin sp1-builder
	cargo +nightly-2024-02-06 build $(FLAGS) --features sp1

sgx:
	@./toolchain.sh sgx
	cargo +stable build $(FLAGS) --features sgx
