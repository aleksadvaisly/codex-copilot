PREFIX ?= $(HOME)/.local
BIN_DIR := $(PREFIX)/bin
TARGET_DIR := ./codex-rs/target/release

.PHONY: install

install:
	cargo build --locked --release --manifest-path ./codex-rs/Cargo.toml -p codex-cli --bin copilot
	mkdir -p "$(BIN_DIR)"
	cp "$(TARGET_DIR)/copilot" "$(BIN_DIR)/copilot"
