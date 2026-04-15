# GitHub Copilot Native Tasks For Codex

## Purpose

Current execution tracker for the native built-in `github-copilot` provider plan
described in `docs/work/GITHUB_COPILOT_NATIVE_PLAN.md`.

This file tracks implementation reality, not just the original order of work.

## Status Legend

- `[x]` implemented and locally verified
- `[ ]` not done or only partially done

## Completed

- [x] Register built-in provider ID `github-copilot`
- [x] Define provider-correct built-in metadata
- [x] Reserve `github-copilot` as a built-in provider ID in config validation
- [x] Implement native GitHub device-code login flow inside `codex-rs/login`
- [x] Persist Copilot auth locally in `github-copilot-auth.json`
- [x] Load Copilot auth on startup
- [x] Add a native runtime bearer-only auth manager for Copilot
- [x] Add `load_github_copilot_session(...)` with runtime bearer refresh
- [x] Add CLI flows for `copilot login`, `copilot login status` and `copilot logout`
- [x] Wire shared standard-turn provider resolution so normal Responses requests can use Copilot session-derived API base URLs
- [x] Capture and parse a real Copilot `/models` payload
- [x] Document and implement the mandatory `/models` IDE headers currently required by Copilot
- [x] Add a typed Copilot `/models` parser without defaulting to loose `Value` parsing
- [x] Add a provider-specific translation layer from Copilot catalog entries to `ModelInfo`
- [x] Filter Copilot catalog entries to picker-visible chat and completion models
- [x] Integrate Copilot model refresh into `codex-rs/models-manager`
- [x] Make the models cache filename provider-aware for Copilot
- [x] Add local tests for the Copilot model translator and Copilot cache filename split
- [x] Make the `copilot` binary default to `~/.codex-copilot` when `CODEX_HOME` is unset
- [x] Persist Copilot `api_base_url` so business-host routing survives across runs
- [x] Add static Copilot IDE headers to the built-in provider metadata used by normal Responses requests

## Implemented But Still Incomplete

- [ ] Copilot auth has native login, storage and session refresh, but it still lacks dedicated tests for the Copilot-specific device flow and refresh failure paths
- [ ] Copilot model discovery is implemented, but missing-field and unknown-field tolerance are not covered explicitly by tests yet
- [ ] Provider-aware login and status UX exists in the dedicated CLI path, but TUI and app-server provider UX is not updated yet

## Remaining Work

- [ ] Prove the canonical standard-turn request path for Copilot, not just `/models`
- [ ] Capture and validate at least one real Copilot text generation stream
- [ ] Prove that the current static-header approach is sufficient for normal Copilot turns or replace it with a dedicated request hook
- [ ] Verify SSE streaming compatibility for Copilot through the normal Codex runtime path
- [ ] Decide whether websocket support stays disabled for MVP or needs dedicated support
- [ ] Add Copilot-specific tests for auth recovery after unauthorized or expired runtime credentials
- [ ] Add Copilot-specific test coverage for the old-auth-file fallback path where `api_base_url` is absent and runtime resolution must refresh it
- [ ] Add Copilot-specific integration tests for `/models` refresh, cache reuse and failure fallback
- [ ] Update TUI and app-server login and status UX so Copilot does not fall through OpenAI-specific messaging
- [ ] Document known limitations, enterprise status and the current request-path assumptions
- [ ] Verify one end-to-end coding turn succeeds with the built-in Copilot provider

## Current Verification

- [x] `cargo test -p codex-arg0`
- [x] `cargo test -p codex-model-provider-info`
- [x] `cargo test -p codex-login`
- [x] `cargo test -p codex-models-manager`
- [x] `cargo test -p codex-core`
- [x] `cargo test -p codex-cli`

## Next Recommended Order

- [ ] 1. Ground the standard Copilot turn request path with a captured request and stream
- [ ] 2. Decide whether the current built-in static header policy is enough or whether a dedicated request hook is required
- [ ] 3. Add Copilot-specific runtime fallback and refresh-path tests
- [ ] 4. Finish TUI and app-server UX so built-in Copilot is product-correct end to end
