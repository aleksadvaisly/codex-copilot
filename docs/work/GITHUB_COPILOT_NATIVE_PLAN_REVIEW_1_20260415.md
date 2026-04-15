# GitHub Copilot Native Provider Plan For Codex -- Review 1

| ID    | Status          | Evidence                                                                                                                                                                                                                                                         |
| ----- | --------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| REQ1  | PASS            | `codex-rs/model-provider-info/src/lib.rs:35-38`, `codex-rs/model-provider-info/src/lib.rs:299-317`, `codex-rs/model-provider-info/src/lib.rs:339-347`, `cargo test -p codex-model-provider-info`                                                                 |
| REQ2  | PARTIAL         | Provider metadata is provider-correct for the built-in registration slice at `codex-rs/model-provider-info/src/lib.rs:299-317`; tests at `codex-rs/model-provider-info/src/model_provider_info_tests.rs:188-204`; native provider-aware UX remains unimplemented |
| REQ3  | NOT_IMPLEMENTED | No native Copilot auth module yet                                                                                                                                                                                                                                |
| REQ4  | NOT_IMPLEMENTED | No native runtime Copilot auth retrieval path yet                                                                                                                                                                                                                |
| REQ5  | NOT_IMPLEMENTED | No Copilot `/models` parser yet                                                                                                                                                                                                                                  |
| REQ6  | NOT_IMPLEMENTED | No `ModelInfo` translator yet                                                                                                                                                                                                                                    |
| REQ7  | NOT_IMPLEMENTED | No `models-manager` Copilot integration yet                                                                                                                                                                                                                      |
| REQ8  | NOT_IMPLEMENTED | No Copilot request customization hook yet                                                                                                                                                                                                                        |
| REQ9  | NOT_IMPLEMENTED | Current TUI option remains a placeholder and does not start native Copilot login                                                                                                                                                                                 |
| REQ10 | PARTIAL         | Spec checklist and implementation plan were added in `docs/work/`; targeted validation passed with `cargo test -p codex-model-provider-info` and `cargo test -p codex-config`; end-to-end Copilot docs and validation are still pending                          |

Convergence score: `0.10` full PASS, `0.20` including PARTIAL items.

## Verification Output

- `just fmt`
- `cargo test -p codex-model-provider-info`
- `cargo test -p codex-config`

All three commands passed for this iteration.
