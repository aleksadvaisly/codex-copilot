use super::anthropic::AnthropicClient;
use super::anthropic::AnthropicOptions;
use crate::auth::AuthProvider;
use crate::common::ResponseStream;
use crate::error::ApiError;
use codex_client::HttpTransport;
use codex_protocol::models::ResponseItem;

/// GitHub Copilot routes Gemini models through the same chat-completions wire
/// shape as Anthropic for now, but this wrapper keeps Gemini as a distinct
/// client entrypoint so the implementation can diverge later without changing
/// call sites again.
pub struct GeminiClient<T: HttpTransport, A: AuthProvider> {
    inner: AnthropicClient<T, A>,
}

/// Gemini-specific options currently match the Anthropic chat-completions options.
pub type GeminiOptions = AnthropicOptions;

impl<T: HttpTransport, A: AuthProvider> GeminiClient<T, A> {
    pub fn new(transport: T, provider: crate::provider::Provider, auth: A) -> Self {
        Self {
            inner: AnthropicClient::new(transport, provider, auth),
        }
    }

    pub fn with_telemetry(
        self,
        request: Option<std::sync::Arc<dyn codex_client::RequestTelemetry>>,
        sse: Option<std::sync::Arc<dyn crate::telemetry::SseTelemetry>>,
    ) -> Self {
        Self {
            inner: self.inner.with_telemetry(request, sse),
        }
    }

    pub async fn stream_request(
        &self,
        model: String,
        system: String,
        input: Vec<ResponseItem>,
        options: GeminiOptions,
    ) -> Result<ResponseStream, ApiError> {
        self.inner
            .stream_request(model, system, input, options)
            .await
    }
}
