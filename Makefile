BIN := envx

.PHONY: build release install uninstall test lint fmt check clean help

build:
	cargo build

release:
	cargo build --release

install:
	cargo install --path .

uninstall:
	cargo uninstall $(BIN)

test:
	cargo test

lint:
	cargo clippy -- -D warnings

fmt:
	cargo fmt

check:
	cargo check

clean:
	cargo clean

help:
	@echo "Usage: make <target>"
	@echo ""
	@echo "  build      Debug build"
	@echo "  release    Optimised release build (LTO + strip)"
	@echo "  install    Install envx to ~/.cargo/bin"
	@echo "  uninstall  Remove envx from ~/.cargo/bin"
	@echo "  test       Run all tests"
	@echo "  lint       Run clippy (warnings as errors)"
	@echo "  fmt        Format source with rustfmt"
	@echo "  check      Fast type-check without producing a binary"
	@echo "  clean      Remove build artifacts"
