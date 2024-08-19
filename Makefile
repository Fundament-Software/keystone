test: build
	cargo test --workspace --all

build:
	CC="$${CC:-clang}" cargo build --workspace --all

.PHONY: build test
