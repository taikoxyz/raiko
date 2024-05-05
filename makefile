TOOLCHAIN_RISC0 = +stable
TOOLCHAIN_SP1 = +nightly-2024-02-06
TOOLCHAIN_SGX = +stable

ifndef DEBUG
	FLAGS = --release
endif

ifdef RUN
	COMMAND = run
else
	COMMAND = build
endif

install:
	./scripts/install.sh $(TARGET)

risc0:
	@cargo $(TOOLCHAIN_RISC0) run --bin risc0-builder
	cargo $(TOOLCHAIN_RISC0) $(COMMAND) $(FLAGS) --features risc0
test_risc0:
	RISC0_DEV_MODE=1 cargo $(TOOLCHAIN_RISC0) test $(FLAGS) -p risc0-driver --features enable

sp1:
	@cargo $(TOOLCHAIN_SP1) run --bin sp1-builder
	cargo $(TOOLCHAIN_SP1) $(COMMAND) $(FLAGS) --features sp1
test_sp1:
	cargo $(TOOLCHAIN_SP1) test $(FLAGS) -p sp1-driver --features enable

sgx:
	cargo $(TOOLCHAIN_SGX) $(COMMAND) $(FLAGS) --features sgx
test_sgx:
	cargo $(TOOLCHAIN_SGX) test $(FLAGS) -p sgx-prover --features enable
