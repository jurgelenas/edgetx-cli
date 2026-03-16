.PHONY: all build build-release test test-verbose lint clean fmt

all: fmt lint test build

build:
	cargo build

build-release:
	cargo build --release

test:
	cargo test

test-verbose:
	cargo test -- --nocapture

lint:
	cargo clippy -- -D warnings

fmt:
	cargo fmt --check

clean:
	cargo clean
