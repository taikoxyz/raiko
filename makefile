
install:
	./script/install.sh $(TARGET)

 # build guest binary only
guest:
	GUEST=1 ./script/build.sh $(TARGET) 

build:
	./script/build.sh $(TARGET) 

run:
	RUN=1 ./script/build.sh $(TARGET)

test:
	TEST=1 ./script/build.sh $(TARGET)
	TEST=1 RUN=1 ./script/build.sh $(TARGET)

integration:
	CONFIG_PATH="config/config.json" ./script/integration.sh $(TARGET)

fmt:
	@cargo fmt --all --check

clippy:
	CLIPPY=1 ./script/build.sh $(TARGET)

update:
	@cargo update
	cd ./provers/sp1/guest && cargo update
	cd ./provers/risc0/guest && cargo update
