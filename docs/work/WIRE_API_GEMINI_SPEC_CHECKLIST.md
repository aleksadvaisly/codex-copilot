# WireApi Gemini -- Checklist

| ID | Requirement | Acceptance Criteria |
|----|-------------|---------------------|
| REQ1 | `WireApi` exposes a `Gemini` variant in provider configuration. | `rg -n "Gemini" codex-rs/model-provider-info/src/lib.rs` shows enum, parsing/display, and tests cover config deserialization. |
| REQ2 | Copilot model discovery marks Gemini-native models as supported by the Gemini wire API without model-name heuristics. | `rg -n "ModelWireApi::Gemini|WireApi::Gemini|gemini" codex-rs/models-manager/src` shows endpoint-driven mapping; tests cover `/models` translation. |
| REQ3 | Core streaming chooses a Gemini-native runtime path based on provider/model wire API. | `rg -n "stream_gemini|StreamRoute::Gemini|WireApi::Gemini|ModelWireApi::Gemini" codex-rs/core/src/client.rs` shows dedicated dispatch path. |
| REQ4 | Gemini remains part of GitHub Copilot provider/auth flow rather than a separate Google auth stack. | No new standalone Google auth flow is added; Gemini path reuses provider credentials and existing Copilot headers/query plumbing. |
| REQ5 | Common conversation model remains Gemini-agnostic above the wire adapter. | Gemini-specific request/response types stay inside wire/client modules; no Gemini-only types leak into UI-facing layers. |
| REQ6 | First iteration supports listing Gemini-native models and completing a basic streamed turn. | Unit/integration tests cover model translation and a streamed text turn over Gemini mock transport. |
| REQ7 | Documentation reflects Gemini as a first-class Copilot wire API and notes first-iteration scope. | `docs/work/WIRE_API_GEMINI.md` or related docs are updated to reflect implementation status and limitations. |

## Known Traps

| TRA-ID | REQ-ID | Trap | Category | Mitigation |
|--------|--------|------|----------|------------|
| TRA1 | REQ2 | `/models` payload may expose Gemini support via endpoint/capability fields that differ from Anthropic. | Boundary/Mismatch | Base mapping on parsed endpoint metadata from live fixtures/tests, not model name prefixes. |
| TRA2 | REQ3 | Gemini streaming event format may differ significantly from Anthropic and Responses. | Implementation Depth | Isolate Gemini serializer and stream processor behind a new route instead of forcing Responses-compatible assumptions. |
| TRA3 | REQ4 | Adding Gemini could accidentally trigger a separate auth path or bypass Copilot-specific headers. | Hidden Prerequisite | Reuse existing provider request builder and auth plumbing; limit Gemini changes to wire selection and payload mapping. |
| TRA4 | REQ6 | Basic text turns may work while tool calls or richer content silently fail. | Scope Reduction / Requirement Non-Compliance | Explicitly keep first iteration text-only unless tool support is implemented and tested; document deferred scope. |
