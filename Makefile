PREFIX ?= $(HOME)/.local
BIN_DIR := $(PREFIX)/bin
TARGET_DIR := ./codex-rs/target/release

.PHONY: install
.PHONY: e2e

install:
	cargo build --locked --release --manifest-path ./codex-rs/Cargo.toml -p codex-cli --bin copilot
	mkdir -p "$(BIN_DIR)"
	cp "$(TARGET_DIR)/copilot" "$(BIN_DIR)/copilot"

e2e:
	cargo test --manifest-path ./codex-rs/Cargo.toml -p codex-app-server --test all suite::v2::copilot_e2e:: -- --ignored
