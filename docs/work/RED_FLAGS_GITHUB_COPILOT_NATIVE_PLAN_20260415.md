# Red Flags: GitHub Copilot Native Provider Plan For Codex

# Date: 20260415

# Iteration: 1

# Review score: convergence=0.10

## 1. Scope Reduction / Requirement Non-Compliance

The new built-in provider is registered, but it is not yet usable as a native Copilot feature. The spec requires native auth, model discovery and working runtime turns, while the code currently only adds provider metadata.

Evidence:

- Spec requires native auth and model discovery: `docs/work/GITHUB_COPILOT_NATIVE_PLAN.md:15-25`, `docs/work/GITHUB_COPILOT_NATIVE_PLAN.md:492-500`
- Current implementation only registers provider metadata: `codex-rs/model-provider-info/src/lib.rs:299-317`, `codex-rs/model-provider-info/src/lib.rs:339-347`
- TUI still exposes a placeholder Copilot sign-in path: `codex-rs/tui/src/onboarding/auth.rs:105`

Severity: HIGH
Impact: Users can eventually select a built-in `github-copilot` provider identity, but they still cannot authenticate and use Copilot natively.

## 2. Lack of Algorithm Grounding

There is still no tested implementation for the hard parts identified by the spec: device auth, token lifecycle, `/models` parsing and request customization.

Evidence:

- Required unit tests are listed in the spec: `docs/work/GITHUB_COPILOT_NATIVE_PLAN.md:438-445`
- The only new tests added in this iteration are provider-registration tests: `codex-rs/model-provider-info/src/model_provider_info_tests.rs:181-204`, `codex-rs/config/src/config_toml.rs:801-821`

Severity: MEDIUM
Impact: The riskiest Copilot-specific logic remains unproven, so most feature risk is still ahead.

## 3. Implicit Logic Changes / Hidden Simplifications

The built-in Copilot provider uses a fixed base URL and disables websockets without a verified upstream contract. That is acceptable for an MVP registration slice, but it is still a policy choice rather than a proven compatibility guarantee.

Evidence:

- Fixed base URL and websocket default: `codex-rs/model-provider-info/src/lib.rs:37-38`, `codex-rs/model-provider-info/src/lib.rs:299-317`
- The spec explicitly says websocket support should be validated and disabled only if messy: `docs/work/GITHUB_COPILOT_NATIVE_PLAN.md:397-407`

Severity: MEDIUM
Impact: If Copilot requires a different base path or benefits from websocket transport, later runtime work may need to revise these defaults.

## Recommendation

Accept this iteration only as groundwork. It is a correct low-risk first slice, but it is not close to feature-complete native Copilot support. The next blocking implementation step is native Copilot auth with persisted token storage and runtime retrieval.
