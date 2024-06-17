
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
	./script/install.sh $(TARGET)
	./script/clippy.sh $(TARGET)
