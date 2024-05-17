
install:
	./script/install.sh $(TARGET)

build:
	./script/build.sh $(TARGET) 

run:
	RUN=1 ./script/build.sh $(TARGET)

test:
	TEST=1 ./script/build.sh $(TARGET)
	TEST=1 RUN=1 ./script/build.sh $(TARGET)

fmt:
	@cargo fmt --all --check

clippy:
	@cargo +nightly-2024-04-18 check --features "sgx,sp1,risc0"
	@cargo +nightly-2024-04-18 clippy --workspace --features "sgx,sp1,risc0" --all-targets -- -D warnings
