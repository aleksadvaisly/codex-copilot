use crate::auth::AuthProvider;
use crate::auth::add_auth_headers;
use crate::common::ResponseEvent;
use crate::common::ResponseStream;
use crate::error::ApiError;
use crate::provider::Provider;
use crate::requests::headers::build_conversation_headers;
use crate::requests::headers::insert_header;
use crate::telemetry::SseTelemetry;
use codex_client::HttpTransport;
use codex_client::RequestCompression;
use codex_client::RequestTelemetry;
use codex_client::StreamResponse;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use http::HeaderMap;
use http::HeaderValue;
use http::Method;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio::time::timeout;
use tracing::debug;
use tracing::instrument;
use tracing::trace;

const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Default)]
pub struct AnthropicOptions {
    pub conversation_id: Option<String>,
    pub extra_headers: HeaderMap,
    pub turn_state: Option<Arc<OnceLock<String>>>,
}

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<AnthropicMessage>,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: String,
    content: Vec<AnthropicContentBlock>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicContentBlock {
    Text { text: String },
}

pub struct AnthropicClient<T: HttpTransport, A: AuthProvider> {
    transport: T,
    provider: Provider,
    auth: A,
    sse_telemetry: Option<Arc<dyn SseTelemetry>>,
}

impl<T: HttpTransport, A: AuthProvider> AnthropicClient<T, A> {
    pub fn new(transport: T, provider: Provider, auth: A) -> Self {
        Self {
            transport,
            provider,
            auth,
            sse_telemetry: None,
        }
    }

    pub fn with_telemetry(
        self,
        _request: Option<Arc<dyn RequestTelemetry>>,
        sse: Option<Arc<dyn SseTelemetry>>,
    ) -> Self {
        Self {
            transport: self.transport,
            provider: self.provider,
            auth: self.auth,
            sse_telemetry: sse,
        }
    }

    #[instrument(
        name = "anthropic.stream_request",
        level = "info",
        skip_all,
        fields(transport = "anthropic_http", http.method = "POST", api.path = "v1/messages")
    )]
    pub async fn stream_request(
        &self,
        model: String,
        system: String,
        input: Vec<ResponseItem>,
        options: AnthropicOptions,
    ) -> Result<ResponseStream, ApiError> {
        let AnthropicOptions {
            conversation_id,
            extra_headers,
            turn_state,
        } = options;

        let messages = anthropic_messages_from_input(input);
        let body = AnthropicRequest {
            model,
            max_tokens: 4_096,
            system,
            messages,
            stream: true,
        };

        let mut headers = extra_headers;
        headers.insert(
            http::header::ACCEPT,
            HeaderValue::from_static("text/event-stream"),
        );
        headers.insert(
            "anthropic-version",
            HeaderValue::from_static(ANTHROPIC_VERSION),
        );
        if let Some(ref conv_id) = conversation_id {
            insert_header(&mut headers, "x-client-request-id", conv_id);
        }
        headers.extend(build_conversation_headers(conversation_id));
        if let Some(turn_state) = turn_state.as_ref()
            && let Some(state) = turn_state.get()
        {
            insert_header(&mut headers, "x-codex-turn-state", state);
        }

        let request = serde_json::to_value(body).map_err(|err| {
            ApiError::Stream(format!("failed to encode anthropic request: {err}"))
        })?;
        let request = add_auth_headers(
            &self.auth,
            codex_client::Request {
                method: Method::POST,
                url: self.provider.url_for_path("v1/messages"),
                headers,
                body: Some(codex_client::RequestBody::Json(request)),
                compression: RequestCompression::None,
                timeout: None,
            },
        );
        let stream_response = self.transport.stream(request).await?;

        Ok(spawn_anthropic_stream(
            stream_response,
            self.provider.stream_idle_timeout,
            self.sse_telemetry.clone(),
        ))
    }
}

fn anthropic_messages_from_input(input: Vec<ResponseItem>) -> Vec<AnthropicMessage> {
    input
        .into_iter()
        .filter_map(|item| match item {
            ResponseItem::Message { role, content, .. } => {
                let text = content
                    .into_iter()
                    .filter_map(|content| match content {
                        ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                            Some(text)
                        }
                        _ => None,
                    })
                    .collect::<String>();
                if text.is_empty() {
                    None
                } else {
                    Some(AnthropicMessage {
                        role,
                        content: vec![AnthropicContentBlock::Text { text }],
                    })
                }
            }
            _ => None,
        })
        .collect()
}

pub fn spawn_anthropic_stream(
    stream_response: StreamResponse,
    idle_timeout: Duration,
    telemetry: Option<Arc<dyn SseTelemetry>>,
) -> ResponseStream {
    let (tx_event, rx_event) = mpsc::channel::<Result<ResponseEvent, ApiError>>(1600);
    tokio::spawn(async move {
        process_anthropic_sse(stream_response.bytes, tx_event, idle_timeout, telemetry).await;
    });
    ResponseStream { rx_event }
}

#[derive(Debug, Deserialize)]
struct AnthropicEvent {
    #[serde(rename = "type")]
    kind: String,
    message: Option<Value>,
    delta: Option<AnthropicDelta>,
}

#[derive(Debug, Deserialize)]
struct AnthropicDelta {
    text: Option<String>,
}

pub async fn process_anthropic_sse(
    stream: codex_client::ByteStream,
    tx_event: mpsc::Sender<Result<ResponseEvent, ApiError>>,
    idle_timeout: Duration,
    telemetry: Option<Arc<dyn SseTelemetry>>,
) {
    let mut stream = stream.eventsource();
    let mut response_id = String::new();
    let mut text = String::new();

    loop {
        let start = Instant::now();
        let response = timeout(idle_timeout, stream.next()).await;
        if let Some(t) = telemetry.as_ref() {
            t.on_sse_poll(&response, start.elapsed());
        }
        let sse = match response {
            Ok(Some(Ok(sse))) => sse,
            Ok(Some(Err(e))) => {
                let _ = tx_event.send(Err(ApiError::Stream(e.to_string()))).await;
                return;
            }
            Ok(None) => {
                let _ = tx_event
                    .send(Err(ApiError::Stream(
                        "stream closed before completion".into(),
                    )))
                    .await;
                return;
            }
            Err(_) => {
                let _ = tx_event
                    .send(Err(ApiError::Stream("idle timeout waiting for SSE".into())))
                    .await;
                return;
            }
        };

        trace!("Anthropic SSE event: {}", &sse.data);
        let event: AnthropicEvent = match serde_json::from_str(&sse.data) {
            Ok(event) => event,
            Err(err) => {
                debug!("failed to parse anthropic SSE event: {err}");
                continue;
            }
        };

        match event.kind.as_str() {
            "message_start" => {
                if let Some(message) = event.message
                    && let Some(id) = message.get("id").and_then(Value::as_str)
                {
                    response_id = id.to_string();
                    let _ = tx_event.send(Ok(ResponseEvent::Created)).await;
                }
            }
            "content_block_delta" => {
                if let Some(delta) = event.delta
                    && let Some(delta_text) = delta.text
                {
                    text.push_str(&delta_text);
                    let _ = tx_event
                        .send(Ok(ResponseEvent::OutputTextDelta(delta_text)))
                        .await;
                }
            }
            "message_delta" => {}
            "message_stop" => {
                let message = ResponseItem::Message {
                    id: Some(response_id.clone()),
                    role: "assistant".to_string(),
                    content: vec![ContentItem::OutputText { text: text.clone() }],
                    end_turn: Some(true),
                    phase: None,
                };
                let _ = tx_event
                    .send(Ok(ResponseEvent::OutputItemDone(message)))
                    .await;
                let _ = tx_event
                    .send(Ok(ResponseEvent::Completed {
                        response_id: response_id.clone(),
                        token_usage: None,
                    }))
                    .await;
                return;
            }
            _ => {}
        }
    }
}
