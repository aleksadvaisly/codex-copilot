# Red Flags: GitHub Copilot Native Provider Plan For Codex

# Date: 20260415

# Iteration: 1

# Review score: convergence=0.60

## 1. Scope Reduction / Requirement Non-Compliance

The standard runtime request path is now wired for Copilot, but the spec still requires proving that a normal coding turn works end to end. The code has no live Copilot streamed-turn test or captured SSE fixture for a real Copilot turn yet.

Evidence:

- Acceptance criteria still require a standard coding turn over the normal runtime path: `docs/work/GITHUB_COPILOT_NATIVE_PLAN.md:496-502`
- Testing plan explicitly calls for one streamed text turn: `docs/work/GITHUB_COPILOT_NATIVE_PLAN.md:446-452`
- Current runtime work resolves Copilot provider access in shared setup, but only local unit and crate tests were run: `codex-rs/core/src/client.rs:662-671`, `codex-rs/cli/src/responses_cmd.rs:35`

Severity: HIGH
Impact: The implementation may still fail against live Copilot Responses streaming even though the local abstraction and crate tests pass.

## 2. Lack of Algorithm Grounding

The Copilot runtime provider resolution falls back to `load_github_copilot_session(...)` when `api_base_url` is missing from stored auth, but there is no direct test proving that fallback path updates both base URL and bearer correctly under a stale old-format auth file.

Evidence:

- Fallback logic is in `codex-rs/login/src/provider_auth.rs:71-90`
- Backward-compatible stored auth shape is introduced in `codex-rs/login/src/copilot_storage.rs:13-19`
- The only new runtime test covers the stored `api_base_url` fast path, not the refresh fallback path: `codex-rs/core/src/client_tests.rs:185-225`

Severity: MEDIUM
Impact: Older Copilot auth files or partially migrated sessions may still exercise an unproven path during the first runtime request.

## 3. Implicit Logic Changes / Hidden Simplifications

Copilot request customization is currently implemented by encoding static IDE headers into the built-in provider definition and resolving the dynamic base URL in shared provider setup. That is smaller than a dedicated request hook, but it also assumes the same header set is valid for normal Responses turns, not just `/models`.

Evidence:

- Static Copilot IDE headers are now baked into built-in provider metadata: `codex-rs/model-provider-info/src/lib.rs:317-340`
- The spec called out a provider-specific request customization hook only if needed and warned that request differences may extend beyond static headers: `docs/work/GITHUB_COPILOT_NATIVE_PLAN.md:386-407`

Severity: MEDIUM
Impact: If live Copilot turn requests require different or additional headers than `/models`, this implementation will need another runtime seam instead of just provider metadata.

## Recommendation

Accept this slice as real progress. Native auth, model discovery and the shared standard-turn runtime path are now connected. Do not call the feature done yet. The next blocking step is live validation of one normal streamed Copilot turn, followed by tests or fixtures that prove the request header policy and fallback refresh path under real runtime conditions.
