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
use codex_protocol::protocol::TokenUsage;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use http::HeaderMap;
use http::HeaderValue;
use http::Method;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
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
    /// Tools to advertise to the model. These are serialized in Chat Completions
    /// format (`{type: "function", function: {name, description, parameters}}`),
    /// which differs from the Responses API format used elsewhere in codex-rs.
    pub tools: Vec<Value>,
}

#[derive(Debug, Serialize)]
struct ChatCompletionsRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<Value>,
}

/// A single message in the Chat Completions message list.
///
/// The `content` field is `Option<String>` because assistant messages that
/// contain tool calls have `content: null` on the wire.
#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    /// Present only on assistant messages that contain tool calls.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<ChatToolCall>,
    /// Present only on tool-result messages (role == "tool").
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

impl ChatMessage {
    fn text(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: Some(content.into()),
            tool_calls: vec![],
            tool_call_id: None,
        }
    }

    fn tool_result(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".to_string(),
            content: Some(content.into()),
            tool_calls: vec![],
            tool_call_id: Some(call_id.into()),
        }
    }

    fn assistant_with_tool_calls(calls: Vec<ChatToolCall>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: None,
            tool_calls: calls,
            tool_call_id: None,
        }
    }
}

#[derive(Debug, Serialize)]
struct ChatToolCall {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    function: ChatToolCallFunction,
}

#[derive(Debug, Serialize)]
struct ChatToolCallFunction {
    name: String,
    arguments: String,
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
            tools,
        } = options;

        let messages = chat_messages_from_input(input, system);
        let body = ChatCompletionsRequest {
            model,
            messages,
            stream: true,
            max_tokens: 4_096,
            tools,
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

/// Converts a slice of `ToolSpec` values into the Chat Completions `tools`
/// array, flattening namespaces and skipping unsupported variants.
/// This conversion is performed in `codex-tools` as
/// `create_tools_json_for_chat_completions` and the result is passed here
/// as a pre-serialized `Vec<Value>` via `AnthropicOptions::tools`.

/// Builds the OpenAI Chat Completions message list from a `Prompt` input.
///
/// Mapping rules:
/// - `system`/`developer` role `Message` items and `base_instructions` are
///   merged into a single leading `system` message.
/// - `user`/`assistant` role `Message` items are forwarded as text messages.
/// - `FunctionCall` items become `assistant` messages with a `tool_calls` array.
/// - `FunctionCallOutput` / `CustomToolCallOutput` items become `tool` messages.
/// - `CustomToolCall` items become `assistant` messages with a `tool_calls` array.
/// - All other item types are silently skipped (no Chat Completions equivalent).
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
        messages.push(ChatMessage::text("system", system_sections.join("\n\n")));
    }

    // Accumulate consecutive FunctionCall / CustomToolCall items so they can
    // be batched into a single assistant message with multiple tool_calls.
    let mut pending_tool_calls: Vec<ChatToolCall> = Vec::new();

    let flush_tool_calls = |pending: &mut Vec<ChatToolCall>, messages: &mut Vec<ChatMessage>| {
        if !pending.is_empty() {
            messages.push(ChatMessage::assistant_with_tool_calls(std::mem::take(
                pending,
            )));
        }
    };

    for item in input {
        match item {
            // system/developer already handled above.
            ResponseItem::Message {
                role, content: _, ..
            } if matches!(role.as_str(), "system" | "developer") => {}

            ResponseItem::Message { role, content, .. } => {
                flush_tool_calls(&mut pending_tool_calls, &mut messages);
                let text = text_from_content(&content);
                if !text.is_empty() {
                    messages.push(ChatMessage::text(role, text));
                }
            }

            // Model-initiated function call (Responses API tool).
            ResponseItem::FunctionCall {
                call_id,
                name,
                arguments,
                ..
            } => {
                pending_tool_calls.push(ChatToolCall {
                    id: call_id,
                    kind: "function".to_string(),
                    function: ChatToolCallFunction { name, arguments },
                });
            }

            // MCP / custom tool call.
            ResponseItem::CustomToolCall {
                call_id,
                name,
                input,
                ..
            } => {
                pending_tool_calls.push(ChatToolCall {
                    id: call_id,
                    kind: "function".to_string(),
                    function: ChatToolCallFunction {
                        name,
                        arguments: input,
                    },
                });
            }

            // Tool result for a Responses API tool call.
            ResponseItem::FunctionCallOutput { call_id, output } => {
                flush_tool_calls(&mut pending_tool_calls, &mut messages);
                let content = output.body.to_text().unwrap_or_default();
                messages.push(ChatMessage::tool_result(call_id, content));
            }

            // Tool result for an MCP / custom tool call.
            ResponseItem::CustomToolCallOutput {
                call_id, output, ..
            } => {
                flush_tool_calls(&mut pending_tool_calls, &mut messages);
                let content = output.body.to_text().unwrap_or_default();
                messages.push(ChatMessage::tool_result(call_id, content));
            }

            // All other variants have no Chat Completions equivalent.
            _ => {}
        }
    }

    flush_tool_calls(&mut pending_tool_calls, &mut messages);

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
    /// Token usage reported on the final chunk (when `stream_options.include_usage`
    /// is set, or unconditionally by some providers including Copilot).
    usage: Option<ChatUsage>,
}

/// Token usage reported by the Chat Completions API on the final SSE chunk.
#[derive(Debug, Deserialize)]
struct ChatUsage {
    prompt_tokens: i64,
    completion_tokens: i64,
    total_tokens: i64,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    delta: Option<ChatDelta>,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatDelta {
    content: Option<String>,
    /// Incremental tool-call fragments. Each element corresponds to one tool
    /// call identified by `index`. The `id` and `function.name` fields arrive
    /// only on the first chunk for a given index; subsequent chunks carry only
    /// `function.arguments` fragments.
    #[serde(default)]
    tool_calls: Vec<ChatToolCallDelta>,
}

#[derive(Debug, Deserialize)]
struct ChatToolCallDelta {
    index: usize,
    id: Option<String>,
    function: Option<ChatToolCallFunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct ChatToolCallFunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

/// In-progress accumulation state for a single tool call being streamed.
#[derive(Debug, Default)]
struct PendingToolCall {
    id: String,
    name: String,
    arguments: String,
}

/// Converts Chat Completions `usage` to the internal `TokenUsage` type.
///
/// Chat Completions reports `prompt_tokens` (input) and `completion_tokens`
/// (output). The `cached_input_tokens` and `reasoning_output_tokens` fields
/// are not available in the standard Chat Completions wire format and default
/// to zero.
fn chat_usage_to_token_usage(u: ChatUsage) -> TokenUsage {
    TokenUsage {
        input_tokens: u.prompt_tokens,
        cached_input_tokens: 0,
        output_tokens: u.completion_tokens,
        reasoning_output_tokens: 0,
        total_tokens: u.total_tokens,
    }
}

/// Processes a GitHub Copilot Chat Completions SSE stream for Claude models.
///
/// The wire format is standard OpenAI streaming:
/// - Each `data:` line carries a `ChatCompletionsChunk` JSON object.
/// - `choices[0].delta.content` carries incremental text.
/// - `choices[0].delta.tool_calls` carries incremental tool-call fragments.
/// - `choices[0].finish_reason == "stop"` signals end-of-turn (text only).
/// - `choices[0].finish_reason == "tool_calls"` signals the model wants to
///   invoke one or more tools.
/// - The final sentinel `data: [DONE]` marks stream close.
///
/// Event sequence emitted for a text turn:
/// 1. `Created`
/// 2. `OutputItemAdded` (empty assistant message)
/// 3. `OutputTextDelta` (one per content chunk)
/// 4. `OutputItemDone` (full assembled message)
/// 5. `Completed`
///
/// Event sequence emitted for a tool-call turn:
/// 1. `Created`
/// 2. For each tool call: `OutputItemAdded(FunctionCall)` then `OutputItemDone(FunctionCall)`
/// 3. `Completed`
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
    // tool_calls_by_index accumulates streaming tool-call fragments.
    let mut tool_calls_by_index: HashMap<usize, PendingToolCall> = HashMap::new();
    // Whether we have seen at least one tool_calls delta (used to distinguish
    // a tool-call turn from a text turn when finish_reason arrives).
    // Token usage from the last chunk that reported it.
    let mut last_usage: Option<ChatUsage> = None;

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
                        token_usage: last_usage.take().map(chat_usage_to_token_usage),
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

        // Accumulate token usage - Copilot reports it on the finish_reason chunk.
        if let Some(usage) = chunk.usage {
            last_usage = Some(usage);
        }

        let choices = chunk.choices.unwrap_or_default();
        let choice = match choices.into_iter().next() {
            Some(c) => c,
            None => continue,
        };

        // --- Accumulate tool-call fragments ---
        if let Some(ref delta) = choice.delta {
            for tc in &delta.tool_calls {
                let entry = tool_calls_by_index.entry(tc.index).or_default();
                if let Some(ref id) = tc.id {
                    entry.id = id.clone();
                }
                if let Some(ref func) = tc.function {
                    if let Some(ref name) = func.name {
                        entry.name = name.clone();
                    }
                    if let Some(ref args) = func.arguments {
                        entry.arguments.push_str(args);
                    }
                }
            }
        }

        // --- Accumulate text fragments ---
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
            match finish.as_str() {
                "tool_calls" => {
                    completed = true;
                    // Emit Created once for the turn.
                    let _ = tx_event.send(Ok(ResponseEvent::Created)).await;

                    // Emit one OutputItemAdded + OutputItemDone per tool call,
                    // ordered by index.
                    let mut indices: Vec<usize> = tool_calls_by_index.keys().copied().collect();
                    indices.sort_unstable();
                    for idx in indices {
                        let tc = tool_calls_by_index.remove(&idx).unwrap_or_default();
                        // Use the index as a tie-breaker for call_id when the
                        // model did not supply one (should not happen in practice).
                        let call_id = if tc.id.is_empty() {
                            format!("call_{idx}")
                        } else {
                            tc.id.clone()
                        };
                        let item = ResponseItem::FunctionCall {
                            id: None,
                            name: tc.name.clone(),
                            namespace: None,
                            arguments: tc.arguments.clone(),
                            call_id: call_id.clone(),
                        };
                        let _ = tx_event
                            .send(Ok(ResponseEvent::OutputItemAdded(item.clone())))
                            .await;
                        let _ = tx_event.send(Ok(ResponseEvent::OutputItemDone(item))).await;
                    }

                    let _ = tx_event
                        .send(Ok(ResponseEvent::Completed {
                            response_id: response_id.clone(),
                            token_usage: last_usage.take().map(chat_usage_to_token_usage),
                        }))
                        .await;
                }
                "stop" | "end_turn" => {
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
                            token_usage: last_usage.take().map(chat_usage_to_token_usage),
                        }))
                        .await;
                    // Continue draining until [DONE] so we don't leave the stream open.
                }
                other => {
                    debug!("unhandled finish_reason: {other}");
                }
            }
        }
    }
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
    use codex_protocol::models::FunctionCallOutputBody;
    use codex_protocol::models::FunctionCallOutputPayload;
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

    /// Fixture with a single tool call (shell command).
    /// Verifies that finish_reason == "tool_calls" produces the correct
    /// Created -> OutputItemAdded(FunctionCall) -> OutputItemDone(FunctionCall) -> Completed sequence.
    #[tokio::test]
    async fn parses_tool_call_sse_fixture() {
        let body = concat!(
            // First chunk: tool call id + name
            "data: {\"id\":\"resp-1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_abc\",\"function\":{\"name\":\"shell\",\"arguments\":\"\"}}]}}]}\n\n",
            // Second chunk: arguments fragment
            "data: {\"id\":\"resp-1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"cmd\\\":\\\"echo hi\\\"}\"}}]}}]}\n\n",
            // Finish chunk
            "data: {\"id\":\"resp-1\",\"choices\":[{\"finish_reason\":\"tool_calls\",\"index\":0,\"delta\":{}}]}\n\n",
            "data: [DONE]\n\n",
        );

        let events = collect_events(body).await;
        let ok_events: Vec<ResponseEvent> = events.into_iter().map(|r| r.unwrap()).collect();

        assert!(
            matches!(ok_events[0], ResponseEvent::Created),
            "first event must be Created"
        );
        assert!(
            matches!(
                &ok_events[1],
                ResponseEvent::OutputItemAdded(ResponseItem::FunctionCall { name, call_id, .. })
                    if name == "shell" && call_id == "call_abc"
            ),
            "second event must be OutputItemAdded(FunctionCall shell), got {:?}",
            ok_events[1]
        );
        assert!(
            matches!(
                &ok_events[2],
                ResponseEvent::OutputItemDone(ResponseItem::FunctionCall { name, arguments, call_id, .. })
                    if name == "shell" && call_id == "call_abc" && arguments.contains("echo hi")
            ),
            "third event must be OutputItemDone(FunctionCall shell with args), got {:?}",
            ok_events[2]
        );
        assert!(
            matches!(ok_events[3], ResponseEvent::Completed { .. }),
            "fourth event must be Completed"
        );
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
            Some("Base instructions\n\nFollow repo policy".to_string())
        );
        assert_eq!(messages[1].role, "user");
        assert_eq!(messages[1].content, Some("hey".to_string()));
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

    #[test]
    fn function_call_and_output_produce_correct_messages() {
        let input = vec![
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "run it".to_string(),
                }],
                end_turn: None,
                phase: None,
            },
            ResponseItem::FunctionCall {
                id: None,
                name: "shell".to_string(),
                namespace: None,
                arguments: "{\"cmd\":\"echo hi\"}".to_string(),
                call_id: "call_1".to_string(),
            },
            ResponseItem::FunctionCallOutput {
                call_id: "call_1".to_string(),
                output: FunctionCallOutputPayload {
                    body: FunctionCallOutputBody::Text("hi\n".to_string()),
                    success: Some(true),
                },
            },
        ];

        let messages = chat_messages_from_input(input, String::new());

        // user, assistant(tool_calls), tool
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].role, "assistant");
        assert!(messages[1].content.is_none());
        assert_eq!(messages[1].tool_calls.len(), 1);
        assert_eq!(messages[1].tool_calls[0].id, "call_1");
        assert_eq!(messages[1].tool_calls[0].function.name, "shell");
        assert_eq!(messages[2].role, "tool");
        assert_eq!(messages[2].tool_call_id, Some("call_1".to_string()));
        assert_eq!(messages[2].content, Some("hi\n".to_string()));
    }

    #[test]
    fn tool_spec_function_serializes_to_chat_completions_format() {
        // This test lives in codex-tools where ToolSpec and JsonSchema are
        // available. See tools/src/tool_spec_tests.rs.
    }
}
