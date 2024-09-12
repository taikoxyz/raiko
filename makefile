
install:
	./script/install.sh $(TARGET)

build:
	./script/build.sh $(TARGET) 

run:
	RUN=1 ./script/build.sh $(TARGET)

test:
	TEST=1 ./script/build.sh $(TARGET)
	TEST=1 RUN=1 ./script/build.sh $(TARGET)

integration:
	./script/integration.sh $(TARGET)

fmt:
	@cargo fmt --all --check

clippy:
	CLIPPY=1 ./script/build.sh $(TARGET)

update:
	@cargo update
	cd ./provers/sp1/guest && cargo update
	cd ./provers/risc0/guest && cargo update
