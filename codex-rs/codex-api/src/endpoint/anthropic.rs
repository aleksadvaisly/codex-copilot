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
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio::time::timeout;
use tracing::debug;
use tracing::instrument;
use tracing::trace;

/// GitHub Copilot routes Claude models through the OpenAI Chat Completions
/// endpoint (`chat/completions`) rather than the native Anthropic
/// `/v1/messages` endpoint, even though the models discovery API advertises
/// `/v1/messages` in `supported_endpoints`. This module implements that
/// Chat Completions wire format for Claude-family models on Copilot.
#[derive(Debug, Default)]
pub struct AnthropicOptions {
    pub conversation_id: Option<String>,
    pub extra_headers: HeaderMap,
    pub turn_state: Option<Arc<OnceLock<String>>>,
}

#[derive(Debug, Serialize)]
struct ChatCompletionsRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    max_tokens: u32,
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
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
        fields(transport = "anthropic_http", http.method = "POST", api.path = "chat/completions")
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

        let messages = chat_messages_from_input(input, system);
        let body = ChatCompletionsRequest {
            model,
            messages,
            stream: true,
            max_tokens: 4_096,
        };

        let mut headers = extra_headers;
        headers.insert(
            http::header::ACCEPT,
            HeaderValue::from_static("text/event-stream"),
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
            ApiError::Stream(format!("failed to encode chat completions request: {err}"))
        })?;
        let mut request_headers = self.provider.headers.clone();
        request_headers.extend(headers);
        let request = add_auth_headers(
            &self.auth,
            codex_client::Request {
                method: Method::POST,
                url: self.provider.url_for_path("chat/completions"),
                headers: request_headers,
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

/// Builds the OpenAI Chat Completions message list from a `Prompt` input.
///
/// `system`/`developer` role items and the `base_instructions` string are
/// merged into a single leading `system` message. All other roles
/// (`user`/`assistant`) are forwarded as-is.
fn chat_messages_from_input(
    input: Vec<ResponseItem>,
    base_instructions: String,
) -> Vec<ChatMessage> {
    // Collect system/developer text from transcript items.
    let mut system_sections: Vec<String> = Vec::new();
    if !base_instructions.is_empty() {
        system_sections.push(base_instructions);
    }
    for item in &input {
        if let ResponseItem::Message { role, content, .. } = item {
            if matches!(role.as_str(), "system" | "developer") {
                let text = text_from_content(content);
                if !text.is_empty() {
                    system_sections.push(text);
                }
            }
        }
    }

    let mut messages: Vec<ChatMessage> = Vec::new();

    if !system_sections.is_empty() {
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: system_sections.join("\n\n"),
        });
    }

    for item in input {
        if let ResponseItem::Message { role, content, .. } = item {
            if matches!(role.as_str(), "system" | "developer") {
                continue;
            }
            let text = text_from_content(&content);
            if !text.is_empty() {
                messages.push(ChatMessage {
                    role,
                    content: text,
                });
            }
        }
    }

    messages
}

fn text_from_content(content: &[ContentItem]) -> String {
    content
        .iter()
        .filter_map(|item| match item {
            ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                Some(text.as_str())
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::chat_messages_from_input;
    use super::process_anthropic_sse;
    use crate::common::ResponseEvent;
    use crate::error::ApiError;
    use bytes::Bytes;
    use codex_client::TransportError;
    use codex_protocol::models::ContentItem;
    use codex_protocol::models::ResponseItem;
    use pretty_assertions::assert_eq;
    use std::time::Duration;
    use tokio::sync::mpsc;

    /// Build a byte stream from raw SSE text, identical to what Copilot sends.
    fn sse_stream(body: &'static str) -> codex_client::ByteStream {
        Box::pin(futures::stream::iter(vec![Ok::<Bytes, TransportError>(
            Bytes::from(body),
        )]))
    }

    /// Collect all events from process_anthropic_sse using a real Chat
    /// Completions fixture captured from the Copilot API.
    async fn collect_events(body: &'static str) -> Vec<Result<ResponseEvent, ApiError>> {
        let (tx, mut rx) = mpsc::channel(64);
        process_anthropic_sse(
            sse_stream(body),
            tx,
            Duration::from_secs(5),
            /*telemetry*/ None,
        )
        .await;
        let mut events = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            events.push(ev);
        }
        events
    }

    /// Fixture captured verbatim from a real `curl` call to
    /// `https://api.githubcopilot.com/chat/completions` with
    /// model `claude-sonnet-4.6`.  Proves the parser handles the exact
    /// wire format without network access.
    #[tokio::test]
    async fn parses_copilot_chat_completions_sse_fixture() {
        let body = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hey\",\"role\":\"assistant\"}}],\"created\":1776340460,\"id\":\"51455fc1-3e01-4051-b782-52642f9f761d\",\"model\":\"claude-sonnet-4.6\"}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"!\",\"role\":\"assistant\"}}],\"created\":1776340460,\"id\":\"51455fc1-3e01-4051-b782-52642f9f761d\",\"model\":\"claude-sonnet-4.6\"}\n\n",
            "data: {\"choices\":[{\"finish_reason\":\"stop\",\"index\":0,\"delta\":{\"content\":null}}],\"created\":1776340460,\"id\":\"51455fc1-3e01-4051-b782-52642f9f761d\",\"usage\":{\"completion_tokens\":7,\"prompt_tokens\":14,\"total_tokens\":21},\"model\":\"claude-sonnet-4.6\"}\n\n",
            "data: [DONE]\n\n",
        );

        let events = collect_events(body).await;
        let ok_events: Vec<ResponseEvent> = events.into_iter().map(|r| r.unwrap()).collect();

        // Expected sequence: Created, OutputItemAdded, delta x2, OutputItemDone, Completed.
        assert!(
            matches!(ok_events[0], ResponseEvent::Created),
            "first event must be Created, got {:?}",
            ok_events[0]
        );
        assert!(
            matches!(
                &ok_events[1],
                ResponseEvent::OutputItemAdded(ResponseItem::Message { role, content, .. })
                    if role == "assistant" && content.is_empty()
            ),
            "second event must be OutputItemAdded(empty assistant), got {:?}",
            ok_events[1]
        );

        let deltas: Vec<&str> = ok_events
            .iter()
            .filter_map(|e| {
                if let ResponseEvent::OutputTextDelta(t) = e {
                    Some(t.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(deltas, vec!["Hey", "!"], "unexpected text deltas");

        assert!(
            ok_events.iter().any(
                |e| matches!(e, ResponseEvent::OutputItemDone(ResponseItem::Message {
                    role,
                    ..
                }) if role == "assistant")
            ),
            "expected OutputItemDone with assistant role"
        );

        let completed = ok_events
            .iter()
            .find(|e| matches!(e, ResponseEvent::Completed { .. }));
        assert!(completed.is_some(), "expected Completed event");
        if let Some(ResponseEvent::Completed { response_id, .. }) = completed {
            assert_eq!(response_id, "51455fc1-3e01-4051-b782-52642f9f761d");
        }
    }

    #[test]
    fn developer_messages_move_to_system_and_do_not_leak_into_messages() {
        let input = vec![
            ResponseItem::Message {
                id: None,
                role: "developer".to_string(),
                content: vec![ContentItem::InputText {
                    text: "Follow repo policy".to_string(),
                }],
                end_turn: None,
                phase: None,
            },
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "hey".to_string(),
                }],
                end_turn: None,
                phase: None,
            },
        ];

        let messages = chat_messages_from_input(input, "Base instructions".to_string());

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "system");
        assert_eq!(
            messages[0].content,
            "Base instructions\n\nFollow repo policy"
        );
        assert_eq!(messages[1].role, "user");
        assert_eq!(messages[1].content, "hey");
    }

    #[test]
    fn no_system_produces_only_user_message() {
        let input = vec![ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: "hello".to_string(),
            }],
            end_turn: None,
            phase: None,
        }];

        let messages = chat_messages_from_input(input, String::new());

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "user");
    }
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

/// Deserialised Chat Completions SSE chunk (streaming delta).
#[derive(Debug, Deserialize)]
struct ChatCompletionsChunk {
    id: Option<String>,
    choices: Option<Vec<ChatChoice>>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    delta: Option<ChatDelta>,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatDelta {
    content: Option<String>,
}

/// Processes a GitHub Copilot Chat Completions SSE stream for Claude models.
///
/// The wire format is standard OpenAI streaming:
/// - Each `data:` line carries a `ChatCompletionsChunk` JSON object.
/// - `choices[0].delta.content` carries incremental text.
/// - `choices[0].finish_reason == "stop"` signals end-of-turn.
/// - The final sentinel `data: [DONE]` marks stream close.
///
/// Event sequence emitted to match what `codex.rs` expects:
/// 1. `Created`
/// 2. `OutputItemAdded` (empty assistant message - initialises `active_item`)
/// 3. `OutputTextDelta` (one per content chunk)
/// 4. `OutputItemDone` (full assembled message)
/// 5. `Completed`
pub async fn process_anthropic_sse(
    stream: codex_client::ByteStream,
    tx_event: mpsc::Sender<Result<ResponseEvent, ApiError>>,
    idle_timeout: Duration,
    telemetry: Option<Arc<dyn SseTelemetry>>,
) {
    let mut stream = stream.eventsource();
    let mut response_id = String::new();
    let mut text = String::new();
    let mut item_opened = false;
    let mut completed = false;

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
                if !completed {
                    let _ = tx_event
                        .send(Err(ApiError::Stream(
                            "stream closed before completion".into(),
                        )))
                        .await;
                }
                return;
            }
            Err(_) => {
                let _ = tx_event
                    .send(Err(ApiError::Stream("idle timeout waiting for SSE".into())))
                    .await;
                return;
            }
        };

        trace!("Copilot chat SSE event: {}", &sse.data);

        if sse.data == "[DONE]" {
            if !completed {
                // Ensure item is always opened before OutputItemDone.
                if !item_opened {
                    let _ = tx_event.send(Ok(ResponseEvent::Created)).await;
                    let _ = tx_event
                        .send(Ok(ResponseEvent::OutputItemAdded(ResponseItem::Message {
                            id: Some(response_id.clone()),
                            role: "assistant".to_string(),
                            content: vec![],
                            end_turn: None,
                            phase: None,
                        })))
                        .await;
                }
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
            }
            return;
        }

        let chunk: ChatCompletionsChunk = match serde_json::from_str(&sse.data) {
            Ok(c) => c,
            Err(err) => {
                debug!("failed to parse chat completions SSE chunk: {err}");
                continue;
            }
        };

        if let Some(id) = chunk.id {
            if response_id.is_empty() {
                response_id = id;
            }
        }

        let choices = chunk.choices.unwrap_or_default();
        let choice = match choices.into_iter().next() {
            Some(c) => c,
            None => continue,
        };

        // On first content chunk: emit Created + OutputItemAdded to initialise
        // `active_item` in the codex state machine before any text deltas.
        if let Some(ref delta) = choice.delta {
            if let Some(ref content) = delta.content {
                if !content.is_empty() {
                    if !item_opened {
                        item_opened = true;
                        let _ = tx_event.send(Ok(ResponseEvent::Created)).await;
                        let _ = tx_event
                            .send(Ok(ResponseEvent::OutputItemAdded(ResponseItem::Message {
                                id: Some(response_id.clone()),
                                role: "assistant".to_string(),
                                content: vec![],
                                end_turn: None,
                                phase: None,
                            })))
                            .await;
                    }
                    text.push_str(content);
                    let _ = tx_event
                        .send(Ok(ResponseEvent::OutputTextDelta(content.clone())))
                        .await;
                }
            }
        }

        if let Some(finish) = choice.finish_reason {
            if finish == "stop" || finish == "end_turn" {
                completed = true;
                // Ensure item is always opened even if there were no content deltas.
                if !item_opened {
                    item_opened = true;
                    let _ = tx_event.send(Ok(ResponseEvent::Created)).await;
                    let _ = tx_event
                        .send(Ok(ResponseEvent::OutputItemAdded(ResponseItem::Message {
                            id: Some(response_id.clone()),
                            role: "assistant".to_string(),
                            content: vec![],
                            end_turn: None,
                            phase: None,
                        })))
                        .await;
                }
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
                // Continue draining until [DONE] so we don't leave the stream open.
            }
        }
    }
}
