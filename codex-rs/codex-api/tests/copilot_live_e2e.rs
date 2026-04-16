//! Live end-to-end test: send "hey" to claude-sonnet-4.6 via the GitHub
//! Copilot API and verify the response stream contains real content.
//!
//! Requirements:
//!   - `~/.codex-copilot/github-copilot-auth.json` must exist with a valid
//!     `github_access_token` field.
//!   - Network access must be available (test is skipped when
//!     `CODEX_SANDBOX_NETWORK_DISABLED=1`).
//!
//! Run with:
//!   cargo test -p codex-api --test copilot_live_e2e -- --ignored
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use codex_api::AnthropicClient;
use codex_api::AnthropicOptions;
use codex_api::AuthProvider;
use codex_api::Provider;
use codex_api::ResponseEvent;
use codex_api::RetryConfig;
use codex_client::ReqwestTransport;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use futures::StreamExt;
use http::HeaderMap;
use http::HeaderValue;
use serde::Deserialize;
use serde_json::Value;

const COPILOT_TOKEN_URL: &str = "https://api.github.com/copilot_internal/v2/token";
const COPILOT_BASE_URL: &str = "https://api.githubcopilot.com";

#[derive(Deserialize)]
struct StoredAuth {
    github_access_token: String,
}

#[derive(Clone)]
struct BearerAuth(String);

impl AuthProvider for BearerAuth {
    fn bearer_token(&self) -> Option<String> {
        Some(self.0.clone())
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn codex_home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").expect("HOME env var not set")).join(".codex-copilot")
}

fn load_github_token() -> Result<String> {
    let path = codex_home().join("github-copilot-auth.json");
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", path.display()))?;
    let auth: StoredAuth = serde_json::from_str(&raw)?;
    Ok(auth.github_access_token)
}

async fn exchange_copilot_token(github_token: &str) -> Result<String> {
    let client = reqwest::Client::new();
    let text = client
        .get(COPILOT_TOKEN_URL)
        .header("Accept", "application/json")
        .header("Authorization", format!("token {github_token}"))
        .header("User-Agent", "codex-rs/0.1")
        .send()
        .await?
        .text()
        .await?;

    let resp: Value = serde_json::from_str(&text).map_err(|e| {
        anyhow::anyhow!("failed to parse copilot token response: {e}\nbody: {text}")
    })?;

    // The exchange endpoint uses {"token": "..."} or {"endpoints":..., "token": "..."}.
    if let Some(t) = resp.get("token").and_then(Value::as_str) {
        return Ok(t.to_string());
    }
    anyhow::bail!("unexpected copilot token response: {resp}")
}

fn copilot_provider() -> Provider {
    let mut headers = HeaderMap::new();
    headers.insert("Editor-Version", HeaderValue::from_static("vscode/1.100.0"));
    headers.insert(
        "Editor-Plugin-Version",
        HeaderValue::from_static("copilot-chat/0.26.0"),
    );
    headers.insert(
        "Copilot-Integration-Id",
        HeaderValue::from_static("vscode-chat"),
    );
    headers.insert(
        "User-Agent",
        HeaderValue::from_static("GitHubCopilotChat/0.26.0"),
    );
    headers.insert(
        "X-GitHub-Api-Version",
        HeaderValue::from_static("2025-10-01"),
    );
    headers.insert(
        "Openai-Intent",
        HeaderValue::from_static("conversation-edits"),
    );

    Provider {
        name: "github-copilot".to_string(),
        base_url: COPILOT_BASE_URL.to_string(),
        query_params: None,
        headers,
        retry: RetryConfig {
            max_attempts: 1,
            base_delay: Duration::from_millis(1),
            retry_429: false,
            retry_5xx: false,
            retry_transport: false,
        },
        stream_idle_timeout: Duration::from_secs(30),
    }
}

fn user_input(text: &str) -> Vec<ResponseItem> {
    vec![ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: text.to_string(),
        }],
        end_turn: None,
        phase: None,
    }]
}

// ── test ──────────────────────────────────────────────────────────────────────

/// Sends "hey" to claude-sonnet-4.6 via Copilot Chat Completions and asserts
/// that the SSE stream delivers at least one non-empty text delta and a
/// Completed event.
#[tokio::test]
#[ignore = "requires network and ~/.codex-copilot/github-copilot-auth.json"]
async fn copilot_claude_hey_returns_content() -> Result<()> {
    if std::env::var("CODEX_SANDBOX_NETWORK_DISABLED").as_deref() == Ok("1") {
        eprintln!("SKIP: network disabled in sandbox");
        return Ok(());
    }

    let github_token = load_github_token()?;
    let copilot_token = exchange_copilot_token(&github_token).await?;

    let transport = ReqwestTransport::new(reqwest::Client::new());
    let client = AnthropicClient::new(transport, copilot_provider(), BearerAuth(copilot_token));

    let mut stream = client
        .stream_request(
            "claude-sonnet-4.6".to_string(),
            String::new(),
            user_input("hey"),
            AnthropicOptions::default(),
        )
        .await?;

    let mut received_text = String::new();
    let mut got_completed = false;

    while let Some(ev) = stream.next().await {
        match ev? {
            ResponseEvent::OutputTextDelta(delta) => {
                received_text.push_str(&delta);
            }
            ResponseEvent::Completed { response_id, .. } => {
                assert!(!response_id.is_empty(), "response_id should not be empty");
                got_completed = true;
                break;
            }
            _ => {}
        }
    }

    assert!(
        !received_text.is_empty(),
        "expected non-empty text from claude-sonnet-4.6, got nothing"
    );
    assert!(got_completed, "expected Completed event");

    println!("claude-sonnet-4.6 replied: {received_text:?}");
    Ok(())
}
