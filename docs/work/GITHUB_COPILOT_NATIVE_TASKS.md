# GitHub Copilot Native Tasks For Codex

## Purpose

Implementation checklist for the native built-in `github-copilot` provider plan
described in:

- `docs/work/GITHUB_COPILOT_NATIVE_PLAN.md`

This is intentionally execution-oriented. It is meant to be worked through in
order and updated as implementation reality changes.

## Ground Rules

- do not start with a full generic provider-auth rewrite
- do not rely on an external proxy as the long-term solution
- keep Copilot-specific logic isolated in dedicated modules or crates where
  practical
- prefer additive changes first, broad refactors only when proven necessary

## Phase 0 - Recon And Validation

- [ ] Confirm the exact GitHub Copilot auth flow to support first.
- [ ] Confirm whether public GitHub and GitHub Enterprise need to be in MVP.
- [ ] Confirm which Copilot endpoint path should be treated as the canonical
      request path for Codex integration.
- [ ] Capture at least one real or fixture-based Copilot `/models` response.
- [ ] Capture at least one real or fixture-based Copilot text generation
      response stream.
- [ ] Document mandatory request headers for Copilot.
- [ ] Document which request fields are accepted, ignored or rejected by
      Copilot.
- [ ] Confirm whether websocket support is worth attempting in MVP.

## Phase 1 - Built-In Provider Definition

- [ ] Add a built-in provider ID `github-copilot` in
      `codex-rs/model-provider-info/src/lib.rs`.
- [ ] Define display name and default provider metadata.
- [ ] Set `wire_api = responses` for Copilot.
- [ ] Decide default `supports_websockets` value for MVP.
- [ ] Decide default base URL strategy.
- [ ] Ensure provider defaults do not pretend Copilot is OpenAI auth.
- [ ] Add tests for built-in provider registration.

## Phase 2 - Native Auth Module

- [ ] Decide whether to implement Copilot auth in a new crate or inside
      `codex-rs/login`.
- [ ] Create module/crate skeleton for Copilot auth.
- [ ] Implement device code request flow.
- [ ] Implement device code polling flow.
- [ ] Implement token exchange completion flow.
- [ ] Implement local token persistence.
- [ ] Implement token loading on startup.
- [ ] Implement token invalidation behavior.
- [ ] Add clear error types for missing, expired and invalid credentials.
- [ ] Add tests for successful login flow.
- [ ] Add tests for timeout and polling errors.
- [ ] Add tests for token persistence and reload.

## Phase 3 - Runtime Auth Integration

- [ ] Identify the smallest native runtime auth abstraction that can represent
      Copilot cleanly.
- [ ] Add a native path for retrieving a Copilot bearer token at request time.
- [ ] Ensure auth retrieval is independent from `requires_openai_auth`.
- [ ] Add retry or invalidation behavior for unauthorized responses.
- [ ] Ensure runtime can distinguish Copilot auth failures from OpenAI auth
      failures.
- [ ] Add integration tests for token reuse across requests.
- [ ] Add integration tests for expired token recovery behavior.

## Phase 4 - Copilot Model Discovery

- [ ] Create a Copilot-specific model discovery module.
- [ ] Add serde types for the raw Copilot `/models` response.
- [ ] Parse the upstream response without using loose `Value` parsing by
      default.
- [ ] Define a translation layer from Copilot models to Codex `ModelInfo`.
- [ ] Decide mapping rules for:
  - [ ] `slug`
  - [ ] `display_name`
  - [ ] `description`
  - [ ] `supported_reasoning_levels`
  - [ ] `visibility`
  - [ ] `shell_type`
  - [ ] `supported_in_api`
  - [ ] `priority`
  - [ ] truncation metadata
  - [ ] tool capability metadata
- [ ] Decide which Copilot models should appear in `/model` by default.
- [ ] Add tests for raw model parsing.
- [ ] Add tests for translation to `ModelInfo`.
- [ ] Add tests for missing-field and unknown-field tolerance.

## Phase 5 - Models Manager Integration

- [ ] Identify the cleanest integration point in
      `codex-rs/models-manager/src/manager.rs`.
- [ ] Add provider-aware model refresh path for `github-copilot`.
- [ ] Reuse existing model cache behavior where possible.
- [ ] Ensure ETag or equivalent cache behavior does not break Copilot support.
- [ ] Ensure `/model` picker receives translated Copilot models.
- [ ] Add tests for initial Copilot model refresh.
- [ ] Add tests for cached model reuse.
- [ ] Add tests for refresh failures and fallback behavior.

## Phase 6 - Request Policy And Headers

- [ ] Create a provider-specific request customization hook.
- [ ] Add static Copilot headers where globally valid.
- [ ] Add dynamic Copilot header logic if request context requires it.
- [ ] Verify request building remains clean for non-Copilot providers.
- [ ] Confirm whether multimodal requests need special Copilot handling.
- [ ] Confirm whether tool-call requests need special Copilot handling.
- [ ] Add tests for Copilot header injection.
- [ ] Add tests that non-Copilot providers are unchanged.

## Phase 7 - Streaming Support

- [ ] Verify HTTP streaming works for standard Copilot text turns.
- [ ] Verify completion and finish-state handling matches Codex expectations.
- [ ] Verify stream error handling and retry behavior.
- [ ] Decide whether websocket support is included in MVP.
- [ ] If yes, add websocket provider support and tests.
- [ ] If no, explicitly disable websocket path for Copilot and test fallback.

## Phase 8 - TUI And User Experience

- [ ] Add provider-aware login trigger for Copilot when credentials are missing.
- [ ] Ensure user-facing copy names `GitHub Copilot` correctly.
- [ ] Ensure error messages do not send the user through ChatGPT/OpenAI recovery.
- [ ] Ensure status views display the active Copilot provider correctly.
- [ ] Ensure `/model` behaves coherently with Copilot-backed catalogs.
- [ ] Add TUI tests for provider-specific login and status states where practical.

## Phase 9 - Config And Documentation

- [ ] Document how to enable and select the built-in Copilot provider.
- [ ] Document where credentials are stored.
- [ ] Document known limitations for MVP.
- [ ] Document enterprise support status.
- [ ] Add release-note style migration notes if provider selection changes.

## Phase 10 - Validation

- [ ] Run unit tests for all new Copilot modules.
- [ ] Run integration tests covering auth, `/models` and standard turns.
- [ ] Verify a standard coding turn succeeds end-to-end.
- [ ] Verify `/model` shows Copilot-backed models correctly.
- [ ] Verify auth failure recovery is understandable.
- [ ] Verify switching away from Copilot does not break existing providers.
- [ ] Verify OpenAI and OSS providers are not regressed.

## Stretch Tasks

- [ ] Add GitHub Enterprise support beyond simple base URL override if needed.
- [ ] Add richer capability mapping for Copilot models.
- [ ] Add tool-call scenario coverage if upstream support is strong enough.
- [ ] Add multimodal support if it is worth the complexity.
- [ ] Generalize provider-auth UX only after Copilot native support is stable.

## Exit Criteria

- [ ] `github-copilot` exists as a built-in provider.
- [ ] Native auth works without external auth scripts.
- [ ] Tokens persist and are reused.
- [ ] Copilot `/models` are translated into working Codex model entries.
- [ ] `/model` works with Copilot.
- [ ] Standard streaming turns work.
- [ ] Error handling is provider-correct and user-understandable.
- [ ] Existing OpenAI and OSS provider flows still work.

## Suggested Implementation Order

- [ ] 1. built-in provider registration
- [ ] 2. native device auth and storage
- [ ] 3. runtime auth integration
- [ ] 4. raw `/models` parsing
- [ ] 5. `ModelInfo` translation
- [ ] 6. models-manager integration
- [ ] 7. provider-specific request hooks
- [ ] 8. TUI/status polish
- [ ] 9. websocket support only if justified
- [ ] 10. full regression verification
