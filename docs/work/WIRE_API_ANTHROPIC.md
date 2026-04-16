# WireApi Anthropic

## Goal

Add first-class Anthropic support to `codex-rs` as a native wire protocol, not as an OpenAI Responses shim.

This document describes:

- what the codebase does today
- what we can realistically graft from `/Users/aleksander/projects/copilot.vim`
- where the current architecture blocks Claude/Gemini-style models
- a concrete implementation plan for `WireApi::Anthropic`

## Why this document exists

Today `codex-rs` is Responses-only at the transport layer.

That is visible in the code:

- `codex-rs/model-provider-info/src/lib.rs:48-55`
  - `WireApi` has only `Responses`
- `codex-rs/core/src/client.rs:1438-1475`
  - `ModelClientSession::stream()` matches only `WireApi::Responses`
- `codex-rs/cli/src/responses_cmd.rs:35-49`
  - raw request path instantiates `codex_api::ResponsesClient`

This means any provider that is not truly usable through an OpenAI-compatible Responses surface is, architecturally, a bad fit right now.

That matches the observed runtime failures with Copilot-exposed models such as:

- `claude-sonnet-4.6`
- `claude-haiku-4.5`
- `gemini-2.5-pro`
- `gpt-4o`

Those failures are not evidence that Anthropic is impossible here. They are evidence that the current wire contract is too narrow.

## Current state in codex-rs

### Transport shape

The codebase assumes one provider abstraction but one actual transport family.

Relevant files:

- `codex-rs/model-provider-info/src/lib.rs`
- `codex-rs/core/src/client.rs`
- `codex-rs/login/src/provider_auth.rs`
- `codex-rs/models-manager/src/manager.rs`

Current facts:

- provider auth is pluggable enough to support provider-specific bearer token handling
- provider metadata is configurable enough to support base URL, headers and auth settings
- request execution is not pluggable enough yet because the runtime path hardcodes Responses

### Places that already branch on wire API

There are small places in the UI and summaries that already ask "is this Responses?"

Examples:

- `codex-rs/tui/src/status/card.rs:260`
- `codex-rs/exec/src/event_processor_with_human_output.rs:438`
- `codex-rs/utils/sandbox-summary/src/config_summary.rs:21`

Today that branch is trivial because there is only one enum variant. Once `Anthropic` exists, these sites need an explicit decision about what settings make sense to show.

### Models pipeline

Remote model discovery is provider-aware only in a narrow way.

Examples:

- `codex-rs/models-manager/src/manager.rs:681-694`
- `codex-rs/models-manager/src/copilot_models.rs`

The Copilot-specific mapper already demonstrates that we sometimes need provider-specific model translation logic. That is useful precedent.

It also currently contains a real bug:

- `codex-rs/models-manager/src/copilot_models.rs:122`
  - every translated Copilot model gets `supported_in_api: true`

That is one reason models appear in the picker and later fail with `unsupported_api_for_model`.

## What copilot.vim teaches us

The useful thing in `/Users/aleksander/projects/copilot.vim` is not the Vim plugin itself.

The useful thing is the bundled language server implementation in:

- `/Users/aleksander/projects/copilot.vim/copilot-language-server/dist/main.js`

Even though it is bundled production JS, it still reveals the architecture.

### Strong signals from the bundled language server

The bundle clearly contains:

- provider families including `OpenAI`, `Anthropic`, `Gemini`, `Groq`, `OpenRouter`, `Azure`
- an `AnthropicMessagesProcessor`
- request construction for Anthropic Messages API
- an `AnthropicProvider`
- separate OpenAI Responses processing

This is the important conclusion:

GitHub's own Copilot language server does not appear to force every provider through one universal OpenAI schema. It has provider-specific machinery where needed.

That is the part worth grafting.

### What is worth grafting

Concepts worth grafting:

- a provider-neutral internal conversation/request model
- provider-specific serializers
- provider-specific stream processors
- provider-specific model discovery
- provider-specific auth and endpoint selection

Concepts not worth grafting:

- direct code from `dist/main.js`
- Vim plugin control flow
- LSP editor integration

Reason:

- the bundle is hard to maintain
- the target codebase is Rust, not JS
- we need maintainable native modules, not a transliteration of minified output

## Architectural recommendation

Do not introduce `llm-chain` or a similar generic orchestration library as the main fix.

Reason:

- this repo needs low-level control over streaming
- it already has provider-specific auth flows
- it uses websocket and SSE behavior directly
- it carries Codex-specific request headers and turn state behavior
- it has tool-calling, compacting and telemetry requirements that are not just "prompt in, text out"

The correct direction is:

- keep a shared internal Codex turn/request model
- add provider-specific wire adapters
- keep provider-specific transport code close to the runtime path

## Target architecture

### New enum variant

Add:

- `WireApi::Anthropic`

in:

- `codex-rs/model-provider-info/src/lib.rs`

That is the smallest correct top-level statement of intent.

### New transport split

The main runtime should stop pretending every provider is a Responses provider.

Desired shape in `codex-rs/core/src/client.rs`:

- `WireApi::Responses` -> existing Responses HTTP/WebSocket path
- `WireApi::Anthropic` -> native Anthropic Messages HTTP/SSE path

Concretely, `ModelClientSession::stream()` should branch to:

- `stream_responses_websocket()` / `stream_responses_api()` for Responses
- new Anthropic path for Messages streaming

### Internal boundary to introduce

Before adding Anthropic, introduce a cleaner boundary inside `core/src/client.rs`.

Suggested split:

- keep `ModelClient` as session-scoped shared state
- add wire-specific request executors

For example:

- `responses_client.rs`
- `anthropic_client.rs`

or equivalent private modules under `core/src/client/`

The current file is large and high-touch. Anthropic should not be appended inline into one massive file if avoidable.

### Shared internal request model

Do not make Anthropic-specific shapes leak all the way up into the app.

Instead:

- keep current `Prompt`, `ModelInfo` and turn-scoped settings as the app-level abstraction
- add a serializer layer that maps those into provider-native requests

For Anthropic this serializer should map:

- Codex system instructions -> Anthropic `system`
- user/assistant/tool transcript -> Anthropic `messages`
- tool schema -> Anthropic tools format
- reasoning-related settings -> Anthropic-compatible options where possible

This is the same broad pattern visible in the Copilot language server bundle.

## Concrete implementation plan

### Phase 1 - Make wire protocol extensible

Files:

- `codex-rs/model-provider-info/src/lib.rs`
- `codex-rs/model-provider-info/src/model_provider_info_tests.rs`
- config/schema docs if affected

Changes:

- add `WireApi::Anthropic`
- update serde parsing and display
- update docs and tests that currently assume only `responses`

Important:

- do not silently reinterpret `anthropic` as `responses`
- make the wire protocol explicit in provider config

### Phase 2 - Isolate Responses-only code paths

Files:

- `codex-rs/core/src/client.rs`
- `codex-rs/cli/src/responses_cmd.rs`

Changes:

- extract Responses-specific request construction and streaming code behind a narrower interface
- keep `responses_cmd` explicitly Responses-only

Why:

- `responses_cmd` is a raw debugging utility for OpenAI-compatible payloads
- it should not become the generic gateway for all wire protocols

Likely outcome:

- `codex responses` remains only for `WireApi::Responses`
- Anthropic gets either no equivalent raw command initially, or a separate dedicated debug command later

### Phase 3 - Add Anthropic request/stream module

New modules, likely in `codex-rs/core/src/client/`:

- `anthropic.rs`
- `anthropic_sse.rs`
- `anthropic_mapping.rs`

Responsibilities:

- build native Messages request bodies
- perform streaming HTTP request
- parse Anthropic SSE events
- map provider-native events into existing Codex `ResponseEvent` stream shape

Key rule:

- the rest of the app should continue consuming Codex-normalized events
- provider-specific differences should be hidden below this layer

### Phase 4 - Add Anthropic provider metadata and auth path

Files:

- `codex-rs/model-provider-info/src/lib.rs`
- `codex-rs/login/src/provider_auth.rs`

Changes:

- add built-in Anthropic provider definition if desired
- or support documented user-defined Anthropic provider config first
- decide auth source shape:
  - likely `env_key = "ANTHROPIC_API_KEY"`
  - no OpenAI auth screen semantics

This part is comparatively easy. The hard part is the transport and event mapping.

### Phase 5 - Add Anthropic model discovery

Files:

- `codex-rs/models-manager/src/manager.rs`
- likely new `codex-rs/models-manager/src/anthropic_models.rs`

Changes:

- native model listing from Anthropic `/v1/models` if we want dynamic discovery
- provider-specific translation into `ModelInfo`

Alternative for first iteration:

- ship a static Anthropic model catalog and defer remote model discovery

This may be the better first cut because it reduces moving parts.

### Phase 6 - Decide capability mapping policy

Need explicit decisions for:

- tool calling support
- image input support
- reasoning controls
- reasoning summaries
- context window metadata
- prompt caching

Do not fake parity.

For each `ModelInfo` field, decide one of:

- supported natively
- degraded / ignored on Anthropic
- unsupported and hidden in UI

### Phase 7 - UI and summary adjustments

Files:

- `codex-rs/tui/src/status/card.rs`
- `codex-rs/exec/src/event_processor_with_human_output.rs`
- `codex-rs/utils/sandbox-summary/src/config_summary.rs`

Changes:

- stop assuming reasoning settings are meaningful for every wire API
- render provider-specific config summaries cleanly

## Suggested first slice

If we want the smallest vertical slice that proves the architecture, do this:

1. add `WireApi::Anthropic`
2. allow a user-defined provider using `wire_api = "anthropic"`
3. support one model, one turn, plain text only
4. no tools yet
5. no websocket path
6. no remote model listing yet

Success criterion:

- simple prompt/response streaming works end-to-end through native Anthropic Messages API

That is the right first proof.

## What not to do

### Do not force Claude through Responses

That recreates the same failure mode we already have with Copilot-exposed models.

### Do not add `llm-chain`

It is the wrong layer for this system.

This codebase already has:

- its own session model
- its own tool/event abstraction
- its own auth and transport behavior

Adding a generic LLM framework here would mostly add translation debt.

### Do not hide provider differences behind fake parity

If Anthropic does not support a given Responses concept in the same way, represent that honestly.

## Graftable ideas from copilot.vim

From the bundled Copilot language server we should copy the architecture, not the code.

Specifically:

- provider enum / provider family split
- provider-specific request builders
- provider-specific SSE processors
- provider-specific model discovery
- provider-specific fetch adapters

That is the proven design pattern.

## Additional notes from aigateway

`/Users/aleksander/projects/aigateway` is a stronger donor than `copilot.vim` for the actual transport design.

Why:

- it is written in Rust
- it has explicit provider crates
- it has a native Anthropic implementation
- it documents the translation boundary clearly

Relevant references:

- `/Users/aleksander/projects/aigateway/README.md`
- `/Users/aleksander/projects/aigateway/docs/translate-design.md`
- `/Users/aleksander/projects/aigateway/providers/aigw-core/src/translate.rs`
- `/Users/aleksander/projects/aigateway/providers/aigw-anthropic/src/translate/request.rs`
- `/Users/aleksander/projects/aigateway/providers/aigw-anthropic/src/translate/stream.rs`
- `/Users/aleksander/projects/aigateway/providers/aigw-openai-compat/src/lib.rs`
- `/Users/aleksander/projects/aigateway/providers/aigw-openai-compat/src/translate.rs`

### What aigateway gets right

The strongest reusable ideas are:

- a clean `RequestTranslator` boundary
- a clean `ResponseTranslator` boundary
- a per-request stateful `StreamParser`
- an HTTP-client-agnostic `TranslatedRequest`
- a thin OpenAI-compat layer driven by explicit capability flags

This is especially important because it matches the actual complexity split:

- OpenAI-compatible providers are mostly transport/header/quirks problems
- Anthropic is a real wire-format problem
- Gemini is a different real wire-format problem

### What to copy from aigateway

Copy the structure, not the crate layout.

The most useful pattern to import into `codex-rs` is:

1. keep Codex's existing app-level canonical types
2. add a translation boundary below them
3. make each wire protocol own its own request mapping and stream parsing

In other words:

- do not replace Codex's app model with `aigateway`'s canonical model
- do adopt `aigateway`'s translator split

### What not to copy from aigateway

Do not import the whole gateway mental model.

`aigateway` is a protocol translation library.
`codex-rs` is a user-facing agent runtime.

So the reusable part is:

- transport seam
- translation seam
- capability seam

Not:

- the entire top-level API shape
- the exact crate decomposition

## Does this also help with Gemini later?

Yes.

Not because Anthropic and Gemini are the same. They are not.

It helps because adding `WireApi::Anthropic` the right way forces us to create the exact extension seam Gemini will also need later.

This is visible in `aigateway`'s own design notes:

- Anthropic and Gemini are both treated as distinct provider-native translation problems
- OpenAI-compatible providers are treated separately through a quirks layer

Gemini specifically will still need its own implementation because its wire model differs in important ways:

- model may live in the URL path, not just the body
- auth may be query-param or custom header based
- content is parts-based rather than plain text/message-first
- role mapping differs
- tool args are object-first
- stream events are snapshot-oriented rather than delta-oriented

So the honest answer is:

- no, Anthropic support does not automatically give us Gemini support
- yes, the same architectural seam makes Gemini much easier to add afterwards

The reusable investment is:

- multi-variant `WireApi`
- wire-specific execution modules
- transport-agnostic translated request boundary
- provider-specific SSE parser boundary
- explicit capability metadata instead of optimistic guessing

If we build that correctly for Anthropic, Gemini becomes the next provider implementation, not the next architecture rewrite.

## Code areas to touch

High confidence areas:

- `codex-rs/model-provider-info/src/lib.rs`
- `codex-rs/core/src/client.rs`
- `codex-rs/login/src/provider_auth.rs`
- `codex-rs/models-manager/src/manager.rs`
- `codex-rs/tui/src/status/card.rs`
- `codex-rs/exec/src/event_processor_with_human_output.rs`
- `codex-rs/utils/sandbox-summary/src/config_summary.rs`

Likely new modules:

- `codex-rs/core/src/client/anthropic.rs`
- `codex-rs/core/src/client/anthropic_mapping.rs`
- `codex-rs/core/src/client/anthropic_sse.rs`
- `codex-rs/models-manager/src/anthropic_models.rs`

## Risks

### Risk 1 - Event model mismatch

Anthropic stream events will not match Responses events 1:1.

Mitigation:

- normalize at the transport boundary
- keep app-facing event types stable

### Risk 2 - Tool schema mismatch

Tool calling semantics are similar enough to tempt direct translation, but different enough to break on edge cases.

Mitigation:

- start text-only
- add tool support in a second step

### Risk 3 - Growing `core/src/client.rs`

This file is already large.

Mitigation:

- extract wire-specific modules before adding Anthropic code

### Risk 4 - Model metadata lies

We already see this with Copilot model mapping.

Mitigation:

- capability flags must be proven by real provider behavior, not inferred optimistically

## Recommendation

Proceed, but do it as a native wire-protocol expansion.

The right architecture is:

- shared Codex session model
- multiple native wire adapters
- provider-specific transport and SSE parsing

The wrong architecture is:

- one universal OpenAI Responses shim for everything
- or a high-level generic LLM library wrapped around that shim

## Immediate next step

If we start implementation, the first code PR should do only this:

1. add `WireApi::Anthropic`
2. refactor `core/src/client.rs` so transport branching is module-friendly
3. no provider behavior change yet

That creates the seam needed for a safe Anthropic follow-up PR.
