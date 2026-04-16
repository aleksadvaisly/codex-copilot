# WireApi Gemini -- Implementation Plan

## REQ1: Add `WireApi::Gemini`
- Extend `codex-rs/model-provider-info/src/lib.rs` with `Gemini` enum variant.
- Update `Display`, serde deserialization, and reasoning-controls behavior.
- Extend provider config tests for `wire_api = "gemini"`.
- Verification: `cargo test -p codex-model-provider-info`.

## REQ2: Model discovery marks Gemini-native models
- Extend wire API detection in `codex-rs/models-manager/src/copilot_models.rs`.
- Infer Gemini support from model endpoint metadata, not model slug naming.
- Extend model translation tests to cover Gemini-capable fixtures.
- Verification: `cargo test -p codex-models-manager`.

## REQ3: Add Gemini streaming route
- Add `StreamRoute::Gemini` and route conversion from provider/model wire API.
- Implement Gemini request serialization and stream-event processing in `codex-rs/core/src/client.rs` or a focused sibling module if extraction is cleaner.
- Reuse existing provider/auth/header plumbing; isolate Gemini-specific payload types locally.
- Verification: `cargo test -p codex-core client_tests::...gemini...`.

## REQ4: Preserve Copilot auth flow
- Ensure Gemini route uses existing Copilot auth context and request builder.
- Avoid introducing separate Google auth/token logic.
- Verification: inspect client setup/tests; no new auth manager paths.

## REQ5: Keep upper layers Gemini-agnostic
- Limit Gemini-specific types to client/wire adapter code.
- Reuse existing `ResponseEvent`/conversation model outputs.
- Verification: `rg -n "Gemini" codex-rs/tui codex-rs/app-server codex-rs/core/src | sed -n '1,120p'` shows only expected wire-selection surfaces.

## REQ6: Cover model list and basic streamed turn
- Add/extend tests for Copilot `/models` mapping.
- Add a mock-stream test for a Gemini text completion turn.
- Keep first iteration scope explicit if tool calling is deferred.
- Verification: targeted crate tests.

## REQ7: Update docs
- Update `docs/work/WIRE_API_GEMINI.md` with status, implemented scope, and deferred items.
- Verification: manual review.
