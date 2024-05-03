
risc0:
	@./toolchain.sh risc0
	@cd provers/risc0/builder/ && cargo +nightly run --release
	cargo build --release --features risc0

sp1:
	@./toolchain.sh sp1
	@cd provers/sp1/builder/ && cargo +nightly run --release
	cargo build --release --features sp1

