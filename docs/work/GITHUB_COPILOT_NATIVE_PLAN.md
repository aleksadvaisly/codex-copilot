# GitHub Copilot Native Provider Plan For Codex

## Goal

Define a native, built-in `GitHub Copilot` provider for `codex`.

This document intentionally excludes the external helper + proxy approach as the
target architecture. It focuses only on what it would take to make Copilot a
first-class provider inside the Codex codebase.

## Scope

This plan assumes the desired end state is:

- `github-copilot` is a built-in model provider
- Codex can authenticate to Copilot without requiring a user-maintained external
  auth script
- Codex can fetch and present Copilot-backed models in `/model`
- Codex can send and stream normal turns through Copilot using the existing
  Codex runtime
- Copilot support feels like a supported product feature, not a local hack

This plan does not assume that onboarding must be fully generalized on day one,
but it does assume that token acquisition, model discovery and request handling
must be native to the repository.

## Bottom Line

This is feasible.

This is a medium-sized feature, not a rewrite.

The main reason it is feasible is that Codex already has:

- a real built-in model provider abstraction
- a Responses-first transport layer
- provider-scoped auth plumbing
- model catalog management

The hardest parts are:

1. native Copilot auth lifecycle
2. Copilot model discovery and translation to Codex `ModelInfo`
3. request/header differences that may require provider-specific hooks

## Architectural Position

### Chosen Direction

Implement `github-copilot` as a built-in provider inside Codex.

That means:

- built-in provider ID in `codex-model-provider-info`
- native auth implementation in repo
- native model discovery and translation in repo
- minimal targeted extensions to request construction where Copilot semantics
  differ from the default OpenAI provider

### Explicitly Rejected For This Plan

#### 1. External proxy as the long-term architecture

Rejected because this document is specifically for the native provider path.

#### 2. Treat Copilot as just another user-defined provider config

Rejected because this does not solve native auth, native discovery or a coherent
product experience.

#### 3. Full provider-auth UX rewrite before MVP

Rejected because that expands scope too early. Copilot support should be proven
first with the smallest native changes that work.

## Why Native Support Fits Codex

Relevant infrastructure already exists.

### Provider Registry

- `codex-rs/model-provider-info/src/lib.rs`
- `codex-rs/core/src/config/mod.rs`

Codex already has a real model provider registry with built-ins plus user
overrides.

### Provider Transport Definition

- `codex-rs/codex-api/src/provider.rs`

Codex already models a provider as:

- base URL
- headers
- query params
- retry policy
- stream timeout

### Provider Auth Plumbing

- `codex-rs/codex-api/src/auth.rs`
- `codex-rs/login/src/provider_auth.rs`
- `codex-rs/login/src/auth/external_bearer.rs`

Codex already understands provider-scoped bearer auth as a concept. Even if the
current implementation is command-backed for custom providers, the architecture
already has a place for provider auth.

### Model Discovery Infrastructure

- `codex-rs/models-manager/src/manager.rs`
- `codex-rs/codex-api/src/endpoint/models.rs`
- `codex-rs/protocol/src/openai_models.rs`

Codex already knows how to fetch, cache and present a remote model catalog.

## Core Design Choice

The central design question is:

Where should Copilot-specific translation live?

There are two plausible answers.

### Option N1 - Translate Inside `models-manager` And Request Layer

Add provider-specific branches in native Codex code:

- model discovery path knows how to fetch Copilot `/models`
- request layer knows how to add Copilot-specific headers or request shaping

This keeps everything in-process and first-class.

### Option N2 - Add A Small Internal Adapter Module

Create a dedicated internal module or crate for Copilot, responsible for:

- auth
- models translation
- request header policy

Then the rest of Codex depends on that adapter.

Recommendation:

- prefer Option N2

Reason:

- cleaner separation
- easier testing
- avoids smearing Copilot conditionals across unrelated code

## Proposed Architecture

## New Native Components

### 1. Built-In Provider Definition

Location:

- `codex-rs/model-provider-info/src/lib.rs`

Add:

- provider ID `github-copilot`
- display name `GitHub Copilot`
- default base URL strategy
- default wire protocol `responses`
- default websocket capability setting if supported

Important:

Do not overload `requires_openai_auth` for Copilot. That field is semantically
wrong for this provider.

### 2. Copilot Auth Module

Suggested location:

- new crate `codex-rs/copilot-login`

Alternative:

- extend `codex-rs/login`

Responsibilities:

- GitHub device authorization flow
- GitHub Enterprise domain support if needed
- local token persistence
- token validity checks
- token retrieval API for Codex runtime

Why a separate crate is attractive:

- keeps GitHub-specific auth logic isolated
- easy to test without polluting generic OpenAI login code
- easier to evolve independently

### 3. Copilot Model Discovery Module

Suggested location:

- new crate `codex-rs/copilot-provider`
- or submodule inside `models-manager`

Responsibilities:

- fetch Copilot model catalog
- parse Copilot schema
- translate to Codex `ModelInfo`
- fill required Codex-specific fields with sane defaults or explicit mappings

### 4. Copilot Request Policy Module

Suggested location:

- same `copilot-provider` crate/module

Responsibilities:

- add required request headers
- choose static vs dynamic headers
- apply provider-specific request tweaks only where needed

This is where logic for things like initiator or intent headers should live if
they cannot be represented purely in provider config.

## Existing Components Likely To Change

### `codex-model-provider-info`

Needed changes:

- add built-in provider definition
- possibly add provider capability flags if current shape is too thin for
  Copilot-specific behavior

### `codex-login`

Needed changes:

- add native auth manager path for Copilot tokens
- or define a new auth source abstraction that is not only OpenAI auth and not
  only external command auth

### `codex-models-manager`

Needed changes:

- add Copilot-specific model refresh path
- or add a provider hook so `github-copilot` can fetch and translate its own
  catalog

### `codex-api`

Possible changes:

- request hooks for provider-specific headers
- optional provider-specific shaping for Responses requests
- websocket handling if Copilot transport differs enough from the default

### `tui`

Likely changes:

- provider-aware login state for Copilot
- status display and onboarding updates
- maybe `/model` messaging when provider-specific discovery fails

## Auth Plan

## Requirements

Native Copilot auth needs to support:

- initial login
- persisted credentials
- non-interactive reuse on later runs
- recovery when token is invalid

## Proposed Design

### Phase A1 - Device Flow First

Start with device authorization, not browser-embedded login.

Reason:

- simpler implementation
- simpler testing
- aligns well with CLI constraints
- good enough for MVP

### Phase A2 - Token Storage

Store Copilot credentials in Codex-managed local storage.

Requirements:

- same security posture as other local auth artifacts
- clear schema for stored data
- support for enterprise metadata if needed later

### Phase A3 - Auth Manager Integration

Add a native way for runtime code to ask for a Copilot bearer token.

This should not be modeled as OpenAI auth and should not depend on
`requires_openai_auth`.

At minimum, Codex needs:

- `AuthSource::Copilot` or equivalent concept
- token getter returning current bearer token
- retry or invalidation path on 401-like failures

## Model Discovery Plan

This is the biggest non-auth task.

## Problem

Codex expects `ModelsResponse` and `ModelInfo`.

Copilot returns a different `/models` schema.

## Proposed Design

### Phase M1 - Parse Raw Copilot Models

Implement a parser for the Copilot catalog response.

This parser should not live inline in generic code.

### Phase M2 - Translate To `ModelInfo`

Create a translator that maps Copilot model metadata into Codex's model schema.

That includes deciding how to populate:

- `slug`
- `display_name`
- `description`
- `supported_reasoning_levels`
- `shell_type`
- `visibility`
- `supported_in_api`
- `priority`
- truncation and capability metadata

Some of these fields will need policy decisions, not raw passthrough.

### Phase M3 - Define Default Mapping Rules

We should explicitly define:

- which Copilot models are shown in `/model`
- which reasoning levels are surfaced
- what defaults apply when upstream metadata is missing

Do not scatter those choices across the codebase.

They should live in one provider-specific mapping module.

## Request Execution Plan

## Problem

Even if Responses transport is broadly compatible, Copilot may still require:

- extra headers
- specific intent values
- special handling for some multimodal or agent cases

## Proposed Design

### Phase R1 - Start With Plain Responses Path

Assume the existing Responses path can handle the majority of request structure.

Only add provider-specific behavior where it is proven necessary.

### Phase R2 - Add Provider-Specific Header Policy

Implement a request customization hook keyed by provider ID.

That hook should be able to:

- inject static headers
- inject dynamic headers based on request context

This is likely the cleanest place for Copilot-specific header logic.

### Phase R3 - Verify Streaming Compatibility

Validate:

- SSE path
- websocket path if applicable
- finish behavior
- tool-call-related chunks

If websocket semantics are messy, disable websockets for Copilot first and ship
HTTP streaming only.

## TUI And UX Plan

## MVP UX Goal

User should be able to select the built-in `github-copilot` provider and use it
without being forced into OpenAI login UX.

## Recommended MVP UX

### U1. No Full Generic Auth Rewrite Yet

Do not start by making onboarding fully generic for all providers.

Instead, add the minimum provider-specific UX for Copilot.

### U2. Provider-Aware Login Entry Point

When the active provider is `github-copilot` and credentials are missing or
invalid, show a provider-specific prompt that starts native Copilot login.

### U3. Status Messaging

Ensure status views and error messages say `GitHub Copilot`, not `OpenAI` or
`ChatGPT`.

## Testing Plan

This feature should not be declared done without provider-specific tests.

### Unit Tests

- device auth request and poll flow
- token persistence and reload
- Copilot model parsing
- translation from Copilot model schema to `ModelInfo`
- request header policy behavior

### Integration Tests

- built-in provider registration
- selecting `github-copilot` as provider
- authenticated `/models` refresh for Copilot
- one streamed text turn
- one auth failure recovery path

### Optional Higher-Value Tests

- enterprise domain support
- websocket enable/disable behavior
- tool-call turn if Copilot supports the chosen request path cleanly

## Risks

### High Risk

- provider-specific request customization turns out to be more extensive than expected
- Copilot model catalog does not map cleanly to Codex `ModelInfo`
- current auth abstractions assume OpenAI too deeply in some runtime paths

### Medium Risk

- TUI login assumptions leak into status and onboarding flows
- websocket transport is not worth supporting initially
- Copilot model metadata changes over time and requires ongoing mapping updates

### Low Risk

- adding a built-in provider ID
- parsing and caching a remote model list in principle
- storing and reusing a bearer token locally

## Recommended Delivery Order

1. Add built-in provider definition.
2. Implement native Copilot device auth and token storage.
3. Integrate Copilot auth retrieval into runtime.
4. Implement raw Copilot `/models` parsing.
5. Implement translation into Codex `ModelInfo`.
6. Wire Copilot model refresh into `models-manager`.
7. Add provider-specific request header policy only where necessary.
8. Add TUI login/status polish.
9. Add websocket support only if it is clearly worth it.

## Acceptance Criteria

Native built-in support is acceptable when:

1. `github-copilot` is available as a built-in provider.
2. User can authenticate natively from Codex without an external auth script.
3. Tokens persist and are reused across runs.
4. `/model` works using a native Copilot-backed catalog.
5. A standard coding turn works over the normal Codex runtime path.
6. Error messages reference Copilot correctly and do not route the user through
   OpenAI-specific recovery steps.

## Recommendation

Proceed with native Copilot support only if the goal is a maintained first-class
provider.

Structure the implementation around a dedicated internal Copilot module or crate,
not scattered provider-specific conditionals across generic code.

The right MVP is:

- built-in provider definition
- native device auth
- native model translation
- minimal provider-specific request hooks

Do not start with a full generic provider-auth overhaul. Add the smallest native
surface that makes `github-copilot` work cleanly, then generalize later if more
hosted providers appear.
