.PHONY: all build test test-verbose lint clean fmt

all: fmt lint test build

build:
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
