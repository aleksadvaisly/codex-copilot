use crate::model_info::BASE_INSTRUCTIONS;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::openai_models::ConfigShellToolType;
use codex_protocol::openai_models::InputModality;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelVisibility;
use codex_protocol::openai_models::ModelWireApi;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::openai_models::ReasoningEffortPreset;
use codex_protocol::openai_models::TruncationPolicyConfig;
use codex_protocol::openai_models::WebSearchToolType;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct CopilotModelsResponse {
    pub(crate) data: Vec<CopilotModel>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CopilotModel {
    pub(crate) id: String,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) capabilities: Option<CopilotCapabilities>,
    #[serde(default)]
    pub(crate) model_picker_enabled: bool,
    #[serde(default)]
    pub(crate) preview: bool,
    #[serde(default)]
    pub(crate) policy: Option<CopilotPolicy>,
    #[serde(default)]
    pub(crate) supported_endpoints: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CopilotCapabilities {
    #[serde(default)]
    pub(crate) limits: Option<CopilotLimits>,
    #[serde(default)]
    pub(crate) supports: Option<CopilotSupports>,
    #[serde(default)]
    pub(crate) r#type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CopilotLimits {
    #[serde(default)]
    pub(crate) max_context_window_tokens: Option<i64>,
    #[serde(default)]
    pub(crate) max_prompt_tokens: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CopilotSupports {
    #[serde(default)]
    pub(crate) parallel_tool_calls: bool,
    #[serde(default)]
    pub(crate) reasoning_effort: Vec<String>,
    #[serde(default)]
    pub(crate) vision: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CopilotPolicy {
    #[serde(default)]
    pub(crate) state: Option<String>,
}

pub(crate) fn translate_models(response: CopilotModelsResponse) -> Vec<ModelInfo> {
    response
        .data
        .into_iter()
        .filter(should_include_model)
        .map(translate_model)
        .collect()
}

fn should_include_model(model: &CopilotModel) -> bool {
    if !model.model_picker_enabled {
        return false;
    }

    if model
        .policy
        .as_ref()
        .and_then(|policy| policy.state.as_deref())
        == Some("disabled")
    {
        return false;
    }

    model
        .capabilities
        .as_ref()
        .and_then(|capabilities| capabilities.r#type.as_deref())
        .is_some_and(|capability_type| capability_type == "chat" || capability_type == "completion")
}

fn translate_model(model: CopilotModel) -> ModelInfo {
    let capabilities = model.capabilities;
    let limits = capabilities
        .as_ref()
        .and_then(|value| value.limits.as_ref());
    let supports = capabilities
        .as_ref()
        .and_then(|value| value.supports.as_ref());
    let supported_reasoning_levels = supports
        .map(|value| translate_reasoning_levels(&value.reasoning_effort))
        .unwrap_or_default();
    let default_reasoning_level = supported_reasoning_levels
        .first()
        .map(|preset| preset.effort);
    let supports_image_detail_original = supports.is_some_and(|value| value.vision);
    let supports_responses_api = model
        .supported_endpoints
        .iter()
        .any(|endpoint| endpoint == "/responses");
    let supports_anthropic_api = model
        .supported_endpoints
        .iter()
        .any(|endpoint| endpoint == "/v1/messages");
    let wire_api = if supports_anthropic_api {
        ModelWireApi::Anthropic
    } else {
        ModelWireApi::Responses
    };
    let context_window =
        limits.and_then(|value| value.max_prompt_tokens.or(value.max_context_window_tokens));
    // Copilot's model discovery API does not always populate `limits` for
    // Anthropic models. Fall back to known context windows so the TUI can
    // display "X% left · YK window" correctly.
    let context_window = context_window.or_else(|| {
        if wire_api == ModelWireApi::Anthropic {
            claude_fallback_context_window(&model.id)
        } else {
            None
        }
    });

    ModelInfo {
        slug: model.id.clone(),
        display_name: model.name,
        description: None,
        default_reasoning_level,
        supported_reasoning_levels,
        shell_type: ConfigShellToolType::Default,
        visibility: ModelVisibility::List,
        supported_in_api: supports_responses_api || supports_anthropic_api,
        priority: if model.preview { 10 } else { 0 },
        additional_speed_tiers: Vec::new(),
        availability_nux: None,
        upgrade: None,
        base_instructions: BASE_INSTRUCTIONS.to_string(),
        model_messages: None,
        supports_reasoning_summaries: false,
        default_reasoning_summary: ReasoningSummary::Auto,
        support_verbosity: false,
        default_verbosity: None,
        apply_patch_tool_type: None,
        web_search_tool_type: WebSearchToolType::Text,
        truncation_policy: TruncationPolicyConfig::bytes(10_000),
        supports_parallel_tool_calls: supports.is_some_and(|value| value.parallel_tool_calls),
        supports_image_detail_original,
        context_window,
        auto_compact_token_limit: None,
        effective_context_window_percent: 95,
        experimental_supported_tools: Vec::new(),
        input_modalities: if supports_image_detail_original {
            vec![InputModality::Text, InputModality::Image]
        } else {
            vec![InputModality::Text]
        },
        used_fallback_model_metadata: false,
        supports_search_tool: false,
        wire_api,
    }
}

/// Returns a known context window size for Claude models exposed through
/// GitHub Copilot when the API does not populate `limits` in its model
/// discovery response.
///
/// Values are sourced from Anthropic's published model documentation.
/// The mapping is intentionally conservative - only models with a confirmed
/// public context window are listed here. Unknown model IDs return `None` so
/// the TUI omits the window display rather than showing a wrong value.
fn claude_fallback_context_window(model_id: &str) -> Option<i64> {
    // All currently-released Claude 3.x / 4.x models share a 200 000-token
    // context window per Anthropic's public documentation.
    if model_id.starts_with("claude-") {
        Some(200_000)
    } else {
        None
    }
}

fn translate_reasoning_levels(levels: &[String]) -> Vec<ReasoningEffortPreset> {
    levels
        .iter()
        .filter_map(|level| {
            let effort = match level.as_str() {
                "none" => ReasoningEffort::None,
                "minimal" => ReasoningEffort::Minimal,
                "low" => ReasoningEffort::Low,
                "medium" => ReasoningEffort::Medium,
                "high" => ReasoningEffort::High,
                "xhigh" => ReasoningEffort::XHigh,
                _ => return None,
            };
            Some(ReasoningEffortPreset {
                effort,
                description: level.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn translates_picker_enabled_chat_models() {
        let response: CopilotModelsResponse = serde_json::from_str(
            r#"{
              "object": "list",
              "data": [
                {
                  "id": "gpt-5.4",
                  "name": "GPT-5.4",
                  "model_picker_enabled": true,
                  "preview": false,
                  "capabilities": {
                    "type": "chat",
                    "limits": {
                      "max_prompt_tokens": 272000
                    },
                    "supports": {
                      "parallel_tool_calls": true,
                      "reasoning_effort": ["low", "medium", "high"],
                      "vision": true
                    }
                  },
                  "supported_endpoints": ["/responses", "ws:/responses"],
                  "policy": {
                    "state": "enabled"
                  }
                },
                {
                  "id": "hidden",
                  "name": "Hidden",
                  "model_picker_enabled": false,
                  "capabilities": {
                    "type": "chat"
                  }
                }
              ]
            }"#,
        )
        .expect("valid response");

        let models = translate_models(response);

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].slug, "gpt-5.4");
        assert_eq!(models[0].display_name, "GPT-5.4");
        assert_eq!(models[0].context_window, Some(272_000));
        assert!(models[0].supported_in_api);
        assert!(models[0].supports_parallel_tool_calls);
        assert!(models[0].supports_image_detail_original);
        assert_eq!(
            models[0]
                .supported_reasoning_levels
                .iter()
                .map(|preset| preset.effort)
                .collect::<Vec<_>>(),
            vec![
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
            ]
        );
    }

    #[test]
    fn marks_messages_api_models_as_supported_in_api() {
        let response: CopilotModelsResponse = serde_json::from_str(
            r#"{
              "object": "list",
              "data": [
                {
                  "id": "claude-sonnet-4.6",
                  "name": "Claude Sonnet 4.6",
                  "model_picker_enabled": true,
                  "capabilities": {
                    "type": "chat",
                    "supports": {
                      "parallel_tool_calls": true,
                      "reasoning_effort": ["low", "medium", "high"],
                      "vision": true
                    }
                  },
                  "supported_endpoints": ["/chat/completions", "/v1/messages"]
                },
                {
                  "id": "gemini-2.5-pro",
                  "name": "Gemini 2.5 Pro",
                  "model_picker_enabled": true,
                  "capabilities": {
                    "type": "chat",
                    "supports": {
                      "parallel_tool_calls": true,
                      "vision": true
                    }
                  },
                  "supported_endpoints": ["/chat/completions"]
                },
                {
                  "id": "gpt-4o",
                  "name": "GPT-4o",
                  "model_picker_enabled": true,
                  "capabilities": {
                    "type": "chat",
                    "supports": {
                      "parallel_tool_calls": true,
                      "vision": true
                    }
                  },
                  "supported_endpoints": ["/chat/completions"]
                }
              ]
            }"#,
        )
        .expect("valid response");

        let models = translate_models(response);

        assert_eq!(models.len(), 3);
        assert_eq!(
            models
                .iter()
                .map(|model| (model.slug.as_str(), model.supported_in_api))
                .collect::<Vec<_>>(),
            vec![
                ("claude-sonnet-4.6", true),
                ("gemini-2.5-pro", false),
                ("gpt-4o", false),
            ]
        );
    }

    #[test]
    fn marks_anthropic_messages_models_as_supported_in_api() {
        let response: CopilotModelsResponse = serde_json::from_str(
            r#"{
              "object": "list",
              "data": [
                {
                  "id": "claude-sonnet-4.6",
                  "name": "Claude Sonnet 4.6",
                  "model_picker_enabled": true,
                  "capabilities": {
                    "type": "chat",
                    "supports": {
                      "parallel_tool_calls": true,
                      "reasoning_effort": ["low", "medium", "high"],
                      "vision": true
                    }
                  },
                  "supported_endpoints": ["/v1/messages"]
                }
              ]
            }"#,
        )
        .expect("valid response");

        let models = translate_models(response);

        assert_eq!(models.len(), 1);
        assert!(models[0].supported_in_api);
    }
}
