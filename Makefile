.PHONY: all build build-release test test-verbose test-integration lint clean fmt

all: fmt lint test build

build:
	cargo build

build-release:
	cargo build --release

test:
	cargo test

test-verbose:
	cargo test -- --nocapture

test-integration:
	cargo test --test simulator_script -- --ignored

lint:
	cargo clippy -- -D warnings

fmt:
	cargo fmt --check

clean:
	cargo clean
