.PHONY: build deploy

build:
	cargo lambda build --release --target aarch64-unknown-linux-gnu

validate:
	sam validate

deploy: validate build
	sam deploy


