
risc0:
	@./toolchain.sh risc0
	@cargo run --bin risc0-builder
	cargo build --release --features risc0

sp1:
	@./toolchain.sh sp1
	@cargo run --bin sp1-builder
	cargo build --release --features sp1

