PREFIX ?= $(HOME)/.local
BIN_DIR := $(PREFIX)/bin
TARGET_DIR := ./codex-rs/target/release

.PHONY: install
.PHONY: run
.PHONY: e2e

run:
	cargo run --locked --manifest-path ./codex-rs/Cargo.toml -p codex-cli --bin copilot

install:
	cargo build --locked --release --manifest-path ./codex-rs/Cargo.toml -p codex-cli --bin copilot
	mkdir -p "$(BIN_DIR)"
	cp "$(TARGET_DIR)/copilot" "$(BIN_DIR)/copilot"

e2e:
	cargo test --manifest-path ./codex-rs/Cargo.toml -p codex-app-server --test all suite::v2::copilot_e2e:: -- --ignored

.PHONY: e2e-model-matrix

e2e-model-matrix:
	cargo test --manifest-path ./codex-rs/Cargo.toml -p codex-app-server --test all suite::v2::copilot_e2e::live_github_copilot_model_matrix_reports_supported_vs_unsupported_models -- --ignored --exact --nocapture
