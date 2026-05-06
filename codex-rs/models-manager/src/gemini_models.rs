use codex_api::AuthProvider;
use codex_api::CoreAuthProvider;
use codex_api::Provider as ApiProvider;
use codex_login::default_client::build_reqwest_client;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CoreResult;
use codex_protocol::openai_models::ConfigShellToolType;
use codex_protocol::openai_models::InputModality;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelVisibility;
use codex_protocol::openai_models::ModelWireApi;
use codex_protocol::openai_models::TruncationPolicyConfig;
use codex_protocol::openai_models::WebSearchToolType;
use http::Method;
use http::header::ACCEPT;
use http::header::HeaderName;
use http::header::HeaderValue;
use serde::Deserialize;
use tokio::time::sleep;

const PAGE_SIZE: u32 = 1_000;
const GENERATE_CONTENT_METHOD: &str = "generateContent";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiModelsResponse {
    models: Vec<GeminiModel>,
    #[serde(default)]
    next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiModel {
    name: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    input_token_limit: Option<i64>,
    #[serde(default)]
    supported_generation_methods: Vec<String>,
}

pub(crate) async fn fetch_models(
    api_provider: &ApiProvider,
    api_auth: &CoreAuthProvider,
) -> CoreResult<Vec<ModelInfo>> {
    let client = build_reqwest_client();
    let mut models = Vec::new();
    let mut page_token: Option<String> = None;

    loop {
        let request = build_request(&client, api_provider, api_auth, page_token.as_deref())?;
        let response = send_with_retry(api_provider, request).await?;
        let body = response.bytes().await.map_err(|err| {
            CodexErr::Fatal(format!("failed to read Gemini models response: {err}"))
        })?;

        let GeminiModelsResponse {
            models: page_models,
            next_page_token,
        } = serde_json::from_slice(&body).map_err(|err| {
            CodexErr::Fatal(format!("failed to decode Gemini models response: {err}"))
        })?;
        models.extend(
            page_models
                .into_iter()
                .filter(should_include_model)
                .map(translate_model),
        );

        match next_page_token {
            Some(token) => page_token = Some(token),
            None => break,
        }
    }

    Ok(models)
}

async fn send_with_retry(
    api_provider: &ApiProvider,
    request: reqwest::RequestBuilder,
) -> CoreResult<reqwest::Response> {
    let max_attempts = api_provider.retry.max_attempts.max(1);
    let mut last_error: Option<String> = None;

    for attempt in 0..max_attempts {
        let Some(attempt_request) = request.try_clone() else {
            return Err(CodexErr::Fatal(
                "failed to clone Gemini models request".to_string(),
            ));
        };
        match attempt_request.send().await {
            Ok(response) if response.status().is_success() => return Ok(response),
            Ok(response) => {
                last_error = Some(format!(
                    "Gemini models request failed with {}",
                    response.status()
                ));
            }
            Err(err) => {
                last_error = Some(err.to_string());
            }
        }

        if attempt + 1 < max_attempts {
            sleep(api_provider.retry.base_delay).await;
        }
    }

    Err(CodexErr::Fatal(last_error.unwrap_or_else(|| {
        "Gemini models request failed".to_string()
    })))
}

fn build_request(
    client: &reqwest::Client,
    api_provider: &ApiProvider,
    api_auth: &CoreAuthProvider,
    page_token: Option<&str>,
) -> CoreResult<reqwest::RequestBuilder> {
    let mut request = client.request(Method::GET, api_provider.url_for_path("models"));
    request = request.headers(api_provider.headers.clone());
    request = request.header(ACCEPT, HeaderValue::from_static("application/json"));

    if let Some((header_name, header_value)) = api_auth.auth_header() {
        let header_name = HeaderName::from_lowercase(header_name.as_bytes())
            .map_err(|err| CodexErr::Fatal(format!("invalid Gemini auth header name: {err}")))?;
        let header_value = HeaderValue::from_str(&header_value)
            .map_err(|err| CodexErr::Fatal(format!("invalid Gemini auth header value: {err}")))?;
        request = request.header(header_name, header_value);
    }

    Ok(request.query(&query_params(page_token)))
}

fn query_params(page_token: Option<&str>) -> Vec<(String, String)> {
    let mut params = vec![("pageSize".to_string(), PAGE_SIZE.to_string())];
    if let Some(page_token) = page_token {
        params.push(("pageToken".to_string(), page_token.to_string()));
    }
    params
}

fn should_include_model(model: &GeminiModel) -> bool {
    model
        .supported_generation_methods
        .iter()
        .any(|method| method == GENERATE_CONTENT_METHOD)
}

fn translate_model(model: GeminiModel) -> ModelInfo {
    let GeminiModel {
        name,
        display_name,
        description,
        input_token_limit,
        supported_generation_methods: _,
    } = model;

    let slug = gemini_model_slug(&name);
    let display_name = display_name.unwrap_or_else(|| slug.clone());
    let context_window = input_token_limit;
    let preview = display_name.to_ascii_lowercase().contains("preview")
        || name.to_ascii_lowercase().contains("preview");

    ModelInfo {
        slug,
        display_name,
        description,
        default_reasoning_level: None,
        supported_reasoning_levels: Vec::new(),
        shell_type: ConfigShellToolType::Default,
        visibility: ModelVisibility::List,
        supported_in_api: true,
        priority: if preview { 10 } else { 0 },
        additional_speed_tiers: Vec::new(),
        availability_nux: None,
        upgrade: None,
        base_instructions: crate::model_info::BASE_INSTRUCTIONS.to_string(),
        model_messages: None,
        supports_reasoning_summaries: false,
        default_reasoning_summary: ReasoningSummary::Auto,
        support_verbosity: false,
        default_verbosity: None,
        apply_patch_tool_type: None,
        web_search_tool_type: WebSearchToolType::Text,
        truncation_policy: context_window
            .map(TruncationPolicyConfig::tokens)
            .unwrap_or_else(|| TruncationPolicyConfig::bytes(10_000)),
        supports_parallel_tool_calls: false,
        supports_image_detail_original: false,
        context_window,
        auto_compact_token_limit: None,
        effective_context_window_percent: 95,
        experimental_supported_tools: Vec::new(),
        input_modalities: vec![InputModality::Text],
        used_fallback_model_metadata: false,
        supports_search_tool: false,
        wire_api: ModelWireApi::Gemini,
    }
}

fn gemini_model_slug(name: &str) -> String {
    name.strip_prefix("models/").unwrap_or(name).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn translates_gemini_models_response() {
        let response: GeminiModelsResponse = serde_json::from_str(
            r#"{
              "models": [
                {
                  "name": "models/gemini-2.5-pro",
                  "displayName": "Gemini 2.5 Pro",
                  "description": "General-purpose model",
                  "inputTokenLimit": 1048576,
                  "outputTokenLimit": 65536,
                  "supportedGenerationMethods": ["generateContent", "countTokens"]
                },
                {
                  "name": "models/gemini-embedding-001",
                  "displayName": "Gemini Embedding 001",
                  "supportedGenerationMethods": ["embedContent"]
                }
              ],
              "nextPageToken": null
            }"#,
        )
        .expect("valid Gemini response");

        let models: Vec<ModelInfo> = response
            .models
            .into_iter()
            .filter(should_include_model)
            .map(translate_model)
            .collect();

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].slug, "gemini-2.5-pro");
        assert_eq!(models[0].display_name, "Gemini 2.5 Pro");
        assert_eq!(models[0].context_window, Some(1_048_576));
        assert_eq!(
            models[0].truncation_policy,
            TruncationPolicyConfig::tokens(1_048_576)
        );
        assert_eq!(models[0].wire_api, ModelWireApi::Gemini);
    }
}
