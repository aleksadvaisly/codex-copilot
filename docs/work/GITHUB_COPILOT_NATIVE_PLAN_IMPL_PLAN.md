# GitHub Copilot Native Provider Plan For Codex -- Implementation Plan

## Iteration 1 Scope

This iteration implements only `REQ1` and the provider-metadata portion of `REQ2`.

Reason:

- this is the lowest-risk native slice already validated by the spec
- it does not require guessing unconfirmed Copilot auth or request contracts
- it unblocks later auth, model-discovery, and UI work by creating a canonical built-in provider ID

## Planned Changes

1. Update `codex-rs/model-provider-info/src/lib.rs`.
   - Add `GITHUB_COPILOT_PROVIDER_ID` and Copilot display-name constants.
   - Add a helper that constructs the built-in Copilot provider entry.
   - Register `github-copilot` inside `built_in_model_providers(...)`.
   - Keep the MVP metadata minimal: `wire_api = responses`, `requires_openai_auth = false`, `supports_websockets = false`.

2. Update `codex-rs/config/src/config_toml.rs`.
   - Treat `github-copilot` as a reserved built-in provider ID so custom config cannot silently override it.

3. Add targeted tests.
   - In `codex-model-provider-info`, add tests that assert the built-in provider exists and its metadata matches the intended MVP shape.
   - In `codex-config`, add a direct unit test that reserved-provider validation rejects a custom `github-copilot` override.

## Trap Mitigations In Scope

- `TRA1`: do not claim runtime compatibility; this iteration only registers the provider.
- `TRA2`: keep `requires_openai_auth = false` and avoid trying to route Copilot through OpenAI login logic.
- `TRA6`: use crate-scoped tests only for touched crates.

## Verification Commands

1. `just fmt`
2. `cargo test -p codex-model-provider-info`
3. `cargo test -p codex-config`

## Out Of Scope For This Iteration

- native Copilot device auth
- runtime bearer retrieval
- `/models` parsing and translation
- request/header customization
- replacing the current TUI Copilot placeholder flow
