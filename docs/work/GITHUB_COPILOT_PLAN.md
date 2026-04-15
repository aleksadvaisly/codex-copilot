# GitHub Copilot Support Plan For Codex

## Goal

Evaluate and define the smallest sane path to add `GitHub Copilot` support to
`codex`.

This document focuses on what the current `codex` architecture already enables,
what still blocks native support and what the recommended delivery path should
be.

## Executive Summary

Compared with `gemini-cli`, `codex` is a much better candidate for Copilot
support.

Reason:

- `codex` already has a real model-provider abstraction
- `codex` already supports Responses-style providers
- `codex` already supports provider-scoped bearer auth via external commands
- `codex` already supports configurable base URLs and extra headers

Because of that, Copilot support in `codex` is not a backend rewrite.

It is still not trivial.

The two main gaps are:

1. Copilot model discovery does not match Codex's expected `/models` schema.
2. Interactive login UX is still OpenAI/ChatGPT-specific.

## Bottom Line

This is feasible.

This is significantly easier in `codex` than in `gemini-cli`.

The easiest path is probably not "patch core first".

The easiest path is:

1. use Codex's existing custom provider support
2. add a Copilot auth helper that outputs a bearer token
3. add a small proxy or adapter for `/models` and possibly request headers

Only after that should we decide whether built-in native provider support is
worth it.

## Why Codex Is A Good Fit

### 1. Real Provider Abstraction Already Exists

Codex has a first-class model-provider system.

Relevant files:

- `codex-rs/model-provider-info/src/lib.rs`
- `codex-rs/core/src/config/mod.rs`
- `codex-rs/codex-api/src/provider.rs`

The provider model already includes:

- `base_url`
- `env_key`
- `auth`
- `wire_api`
- `http_headers`
- `env_http_headers`
- retry and timeout configuration
- `supports_websockets`

This is already the right architectural shape for a Copilot integration.

### 2. Responses API Is Already The Main Wire Contract

Codex is already oriented around a Responses-style backend contract.

Relevant files:

- `codex-rs/model-provider-info/src/lib.rs`
- `codex-rs/core/src/client.rs`
- `codex-rs/codex-api/src/lib.rs`

That matters because Copilot is much closer to an OpenAI-style responses/chat
integration than `gemini-cli` is.

### 3. Provider-Scoped Auth Already Exists

Codex already supports provider-specific bearer-token acquisition through a
command.

Relevant files:

- `codex-rs/protocol/src/config_types.rs`
- `codex-rs/login/src/provider_auth.rs`
- `codex-rs/login/src/auth/external_bearer.rs`

This is important because it means Copilot auth can start as an external helper
without forcing an immediate TUI auth rewrite.

### 4. Local OSS Providers Prove The Pattern

Relevant crates:

- `codex-rs/ollama`
- `codex-rs/lmstudio`

These show that Codex already supports multiple model backends and provider-
specific environment preparation.

The presence of `ollama` is a good sign, not because Copilot works like Ollama,
but because it proves the system was built to support more than a single hosted
backend.

## What Already Works In Favor Of Copilot

### Custom Provider Configuration

Codex can merge built-in and user-defined providers at runtime.

Relevant code:

- `codex-rs/core/src/config/mod.rs`

That means a Copilot integration does not have to start as a built-in provider.

### External Bearer Auth

Codex can run a command, capture stdout and treat it as the provider token.

Relevant code:

- `codex-rs/login/src/auth/external_bearer.rs`

That means a helper executable or script can do:

- device login
- cached token retrieval
- token refresh or re-login

and `codex` can consume the resulting bearer token without knowing the full auth
mechanics.

### Static And Environment Headers

Codex provider definitions can already inject custom headers.

Relevant code:

- `codex-rs/model-provider-info/src/lib.rs`

This is useful because Copilot often needs specific request headers beyond
standard bearer auth.

### Models Refresh Infrastructure

Codex already has a `ModelsManager` that fetches `/models`, caches it and wires
it into the picker/runtime.

Relevant code:

- `codex-rs/models-manager/src/manager.rs`
- `codex-rs/codex-api/src/endpoint/models.rs`

This is good infrastructure, but it also exposes the main mismatch described
below.

## Main Gaps

### Gap 1 - `/models` Shape Mismatch

This is the biggest practical problem.

Codex expects `/models` to return its own `ModelsResponse` and `ModelInfo`
shapes.

Relevant files:

- `codex-rs/codex-api/src/endpoint/models.rs`
- `codex-rs/protocol/src/openai_models.rs`

GitHub Copilot does not return that shape.

So while a raw Copilot backend may be close enough for requests, model discovery
is not plug-and-play.

This means at least one of the following is required:

1. a proxy that rewrites Copilot `/models` into Codex `ModelsResponse`
2. native Copilot model discovery and translation inside Codex
3. a mode that disables remote `/models` and uses a static provider-specific
   catalog

### Gap 2 - Login UX Is Still OpenAI-Specific

Codex onboarding and login screens are still built around:

- ChatGPT login
- OpenAI API key login

Relevant files:

- `codex-rs/tui/src/onboarding/auth.rs`
- `codex-rs/tui/src/lib.rs`
- `codex-rs/model-provider-info/src/lib.rs`

In particular, the current onboarding logic checks `requires_openai_auth`, which
is not a generic provider-login concept.

So even though provider auth exists at runtime, the interactive UX is not yet
generalized for Copilot.

### Gap 3 - Copilot-Specific Headers And Semantics

Some Copilot requests need extra headers or behavior, for example around:

- initiator type
- intent headers
- vision signaling

Codex can already add fixed headers, but if some of those headers need to change
per request or depend on payload shape, a simple static provider config may not
be enough.

That would push us toward a proxy or native built-in provider logic.

## Delivery Options

## Option A - External Helper + Proxy + Custom Provider

### Summary

This is the fastest and least invasive path.

Build three pieces:

1. a token helper that logs into GitHub Copilot and prints a bearer token
2. a small proxy that adapts Copilot to what Codex expects
3. a `model_providers.github-copilot` config entry

### Why This Is Attractive

- avoids changing TUI onboarding first
- avoids changing built-in provider lists first
- reuses `auth.command`
- keeps Copilot logic out of core until it proves useful

### Required Components

#### A1. Token Helper

Standalone tool or crate that:

- initiates GitHub device login
- caches the token locally
- prints the current bearer token to stdout

This plugs directly into:

- `ModelProviderAuthInfo`
- `external_bearer_only`

#### A2. Copilot-Codex Proxy

A local proxy process that:

- accepts Codex requests
- forwards `responses` calls to Copilot
- rewrites or serves `/models` in Codex format
- injects Copilot-specific request headers if needed

The existing crate:

- `codex-rs/responses-api-proxy`

is a useful starting point, but it currently only proxies `/v1/responses`. It is
not enough by itself because Codex also expects `/models`.

#### A3. Provider Config

Example shape:

```toml
model_provider = "github-copilot"

[model_providers.github-copilot]
name = "GitHub Copilot"
base_url = "http://127.0.0.1:8787/v1"
wire_api = "responses"
supports_websockets = false

[model_providers.github-copilot.auth]
command = "/path/to/copilot-auth-helper"
args = ["token"]
```

Additional static headers can be added if they are globally valid.

### Complexity

Low to medium.

This is probably the best MVP path.

## Option B - Built-In Native Provider

### Summary

Add `github-copilot` as a first-class built-in provider in Codex.

### What This Requires

1. built-in provider registration in `codex-model-provider-info`
2. Copilot auth helper inside repo
3. Copilot model discovery and translation into `ModelInfo`
4. Copilot-specific header logic where needed
5. maybe provider-specific tests for streaming and `/models`

### Benefits

- cleaner user experience
- no external proxy required if Codex directly supports Copilot request/response needs
- easier documentation and support

### Costs

- wider code changes
- still does not fully solve onboarding unless that is also extended

### Complexity

Medium.

This is the best product-quality path if Copilot support is expected to stay.

## Option C - Full Generic Provider Login UX

### Summary

Generalize Codex login UX so providers can expose their own auth modes:

- browser login
- device code login
- token-based login

### Why This Exists

Right now, interactive auth UX is OpenAI-centric.

If Codex wants more than one serious hosted provider, this generic auth layer
will eventually be needed.

### Complexity

Medium to high.

This is not the right MVP unless there is already a product push for broader
multi-provider support.

## Recommended Direction

Recommendation:

1. Start with Option A.
2. Validate that Copilot actually works well enough through the Responses path.
3. If adoption is real, move to Option B.
4. Only do Option C if multi-provider interactive auth becomes a broader product
   need.

This sequence minimizes risk and avoids overcommitting before the request and
model semantics are proven.

## Concrete MVP Plan

### Phase 1 - Verify Copilot Request Compatibility

Before writing a lot of code, verify these assumptions against live Copilot or a
captured fixture:

1. Can Codex-style Responses requests be accepted with minimal translation?
2. Which headers are mandatory?
3. Are there request fields that Copilot rejects?
4. Does streaming behavior match what Codex expects well enough?

If the answer is "mostly yes", continue.

If the answer is "no", the proxy becomes mandatory.

### Phase 2 - Build Auth Helper

Implement a small tool that:

- performs device authorization with GitHub
- persists token locally
- prints valid token to stdout

This tool should be independent from Codex core.

### Phase 3 - Build Proxy MVP

Implement a proxy that supports:

- `POST /v1/responses`
- `GET /v1/models`

Responsibilities:

- forward Responses requests to Copilot
- add Copilot-required headers
- translate model list into Codex `ModelsResponse`

If the upstream path is not exactly `/v1/responses`, normalize it in the proxy so
Codex can remain unchanged.

### Phase 4 - Add Config Example

Add a documented custom provider example showing:

- provider entry
- auth command
- base URL
- optional static headers

This may be enough for early testers without any core code changes.

### Phase 5 - Evaluate Native Integration

After MVP validation, decide whether to:

- keep helper + proxy as the integration path
- or promote it to a built-in provider

## Native Integration Plan

If we decide to make Copilot first-class inside Codex, the likely steps are:

### N1. Add Built-In Provider Definition

Touch:

- `codex-rs/model-provider-info/src/lib.rs`

Add:

- provider ID `github-copilot`
- default base URL strategy
- auth strategy choice
- header defaults where possible

### N2. Add Copilot Auth Crate Or Module

Possible location:

- `codex-rs/login` extension
- or new crate like `codex-rs/copilot-login`

Responsibilities:

- device code login
- enterprise domain support
- token storage
- token read command for provider auth

### N3. Add Model Translation Layer

Possible locations:

- `codex-rs/models-manager`
- `codex-rs/model-provider-info`
- dedicated new crate

Responsibilities:

- fetch Copilot model catalog
- translate into `ModelInfo`
- choose sane defaults for unsupported Codex-specific metadata

### N4. Add Optional Request Header Hooks

If Copilot needs dynamic request headers based on request type, static provider
headers will not be enough.

At that point, request construction may need provider-specific hooks in:

- `codex-rs/codex-api`
- or a proxy retained even for built-in support

### N5. Extend Onboarding If Needed

Only if native UX matters, generalize onboarding so non-OpenAI hosted providers
can offer their own login methods.

## Risks

### High Risk

- Copilot request semantics diverge more than expected from Codex Responses usage
- model discovery translation is more opinionated than it looks
- some Copilot headers need dynamic logic, not static config

### Medium Risk

- model metadata required by Codex does not map cleanly from Copilot
- websocket support is partial or not worth using
- enterprise Copilot endpoints differ from github.com behavior

### Low Risk

- command-based token retrieval in principle
- custom provider registration
- static header injection

## Why `/models` Is The Main Friction Point

Codex already knows how to talk to providers over Responses.

What it does not get for free is Codex's richer model catalog contract. The
Codex `/models` schema carries fields like:

- visibility
- shell type
- supported reasoning levels
- picker metadata
- truncation policy
- tool support flags

Copilot's model list does not naturally speak that schema.

So any real integration must decide where that translation lives:

- proxy
- built-in provider adapter
- static catalog

That is the central design choice.

## Suggested Acceptance Criteria For MVP

MVP is acceptable when:

1. Codex can authenticate to Copilot through `auth.command`.
2. Codex can call Copilot through a configured provider.
3. `/model` picker works using a translated model catalog.
4. A normal coding turn streams successfully.
5. Errors are understandable when the token is expired or missing.
6. No OpenAI login flow needs to be used for the Copilot provider.

## Suggested Acceptance Criteria For Native Built-In Support

Native support is acceptable when:

1. `github-copilot` can be selected like other providers.
2. Token retrieval is documented and stable.
3. Model discovery works without external manual patching.
4. Common turns work without a separate external proxy, or the proxy is bundled
   and invisible to the user.
5. TUI status and `/model` behavior remain coherent.

## Recommendation

Do not start by rewriting Codex auth UX.

Do not start by forcing Copilot directly into the existing OpenAI login flow.

Start with a helper plus proxy MVP because Codex already gives us the hooks we
need:

- custom provider config
- bearer token command auth
- configurable base URL
- configurable headers
- existing Responses transport

That is the minimum-risk path and the fastest way to learn whether a deeper
native integration is worth doing.
