#![expect(clippy::expect_used)]

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use app_test_support::McpProcess;
use app_test_support::to_response;
use codex_app_server_protocol::Account;
use codex_app_server_protocol::GetAccountParams;
use codex_app_server_protocol::GetAccountResponse;
use codex_app_server_protocol::ItemCompletedNotification;
use codex_app_server_protocol::JSONRPCError;
use codex_app_server_protocol::JSONRPCMessage;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::ModelListParams;
use codex_app_server_protocol::ModelListResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_login::load_github_copilot_session;
use reqwest::header::ACCEPT;
use reqwest::header::AUTHORIZATION;
use reqwest::header::HeaderMap as ReqwestHeaderMap;
use reqwest::header::HeaderValue;
use serde::Deserialize;
use tempfile::TempDir;
use tokio::time::timeout;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);
const COPILOT_EDITOR_PLUGIN_VERSION: &str = "copilot.vim/1.59.0";
const COPILOT_EDITOR_VERSION: &str = "Vim/9.1.1752";
const COPILOT_INTEGRATION_ID: &str = "vscode-chat";
const COPILOT_LANGUAGE_SERVER_VERSION: &str = "1.408.0";

const MODEL_MATRIX_CANDIDATES: &[&str] = &[
    "gpt-5.4",
    "claude-sonnet-4.6",
    "claude-haiku-4.5",
    "gemini-2.5-pro",
    "gpt-4o",
    "gemini-3.1-pro-preview",
];

#[derive(Debug)]
enum HeyTurnOutcome {
    Completed(TurnCompletedNotification),
    UnsupportedByResponsesApi(String),
    Failed(TurnCompletedNotification),
    RpcError(JSONRPCError),
}

#[derive(Debug, Deserialize)]
struct RawCopilotModelsResponse {
    data: Vec<RawCopilotModel>,
}

#[derive(Debug, Deserialize)]
struct RawCopilotModel {
    id: String,
    #[serde(default)]
    supported_endpoints: Vec<String>,
}

fn is_responses_api_unsupported(message: &str) -> bool {
    message.contains("unsupported_api_for_model")
        || message.contains("does not support Responses API")
}

fn require_copilot_home() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let path = PathBuf::from(home).join(".codex-copilot");
    path.join("github-copilot-auth.json")
        .exists()
        .then_some(path)
}

fn write_live_copilot_config(codex_home: &TempDir) -> Result<()> {
    std::fs::write(
        codex_home.path().join("config.toml"),
        r#"
model_provider = "github-copilot"
approval_policy = "never"
sandbox_mode = "read-only"

[features]
shell_snapshot = false
"#,
    )?;
    Ok(())
}

fn copy_live_copilot_auth(source_home: &PathBuf, codex_home: &TempDir) -> Result<()> {
    let auth_src = source_home.join("github-copilot-auth.json");
    let auth_dest = codex_home.path().join("github-copilot-auth.json");
    std::fs::copy(&auth_src, &auth_dest)
        .with_context(|| format!("copy {} -> {}", auth_src.display(), auth_dest.display()))?;
    Ok(())
}

async fn live_model_list(mcp: &mut McpProcess) -> Result<ModelListResponse> {
    let request_id = mcp
        .send_list_models_request(ModelListParams {
            cursor: None,
            limit: Some(100),
            include_hidden: Some(true),
        })
        .await?;
    let response: JSONRPCResponse = timeout(
        DEFAULT_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    to_response::<ModelListResponse>(response)
}

async fn live_raw_copilot_models(codex_home: &TempDir) -> Result<RawCopilotModelsResponse> {
    let session = load_github_copilot_session(codex_home.path())
        .await?
        .context("expected GitHub Copilot session")?;

    let client = codex_login::default_client::build_reqwest_client();
    let mut headers = ReqwestHeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", session.access_token))?,
    );
    headers.insert(
        "X-GitHub-Api-Version",
        HeaderValue::from_static("2025-10-01"),
    );
    headers.insert(
        "Editor-Version",
        HeaderValue::from_static(COPILOT_EDITOR_VERSION),
    );
    headers.insert(
        "Editor-Plugin-Version",
        HeaderValue::from_static(COPILOT_EDITOR_PLUGIN_VERSION),
    );
    headers.insert(
        "Copilot-Language-Server-Version",
        HeaderValue::from_static(COPILOT_LANGUAGE_SERVER_VERSION),
    );
    headers.insert(
        "User-Agent",
        HeaderValue::from_static("GithubCopilot/1.408.0"),
    );
    headers.insert(
        "Copilot-Integration-Id",
        HeaderValue::from_static(COPILOT_INTEGRATION_ID),
    );

    let payload = client
        .get(format!(
            "{}/models",
            session.api_base_url.trim_end_matches('/')
        ))
        .headers(headers)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    serde_json::from_str(&payload).context("GitHub Copilot /models payload should deserialize")
}

async fn start_thread_with_model(
    mcp: &mut McpProcess,
    model: String,
) -> Result<ThreadStartResponse> {
    let thread_request_id = mcp
        .send_thread_start_request(ThreadStartParams {
            model: Some(model),
            ..Default::default()
        })
        .await?;
    let thread_response: JSONRPCResponse = timeout(
        DEFAULT_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(thread_request_id)),
    )
    .await??;
    to_response::<ThreadStartResponse>(thread_response)
}

async fn send_hey_turn_and_wait(mcp: &mut McpProcess, thread_id: String) -> Result<HeyTurnOutcome> {
    let turn_request_id = mcp
        .send_turn_start_request(TurnStartParams {
            thread_id,
            input: vec![UserInput::Text {
                text: "hey".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;

    let deadline = tokio::time::Instant::now() + DEFAULT_TIMEOUT;
    let request_id = RequestId::Integer(turn_request_id);
    let mut response_seen = false;

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let message = timeout(remaining, mcp.read_next_message()).await??;
        match message {
            JSONRPCMessage::Response(response) if response.id == request_id => {
                response_seen = true;
            }
            JSONRPCMessage::Error(error) if error.id == request_id => {
                if is_responses_api_unsupported(&error.error.message) {
                    return Ok(HeyTurnOutcome::UnsupportedByResponsesApi(
                        error.error.message,
                    ));
                }
                return Ok(HeyTurnOutcome::RpcError(error));
            }
            JSONRPCMessage::Notification(notification)
                if response_seen && notification.method == "turn/completed" =>
            {
                let completed: TurnCompletedNotification = serde_json::from_value(
                    notification
                        .params
                        .expect("turn/completed params must be present"),
                )?;
                return Ok(match completed.turn.status {
                    TurnStatus::Completed => HeyTurnOutcome::Completed(completed),
                    TurnStatus::Failed => {
                        let error_message = completed.turn.error.as_ref().map(|error| match &error
                            .additional_details
                        {
                            Some(details) => format!("{} ({details})", error.message),
                            None => error.message.clone(),
                        });

                        match error_message {
                            Some(message) if is_responses_api_unsupported(&message) => {
                                HeyTurnOutcome::UnsupportedByResponsesApi(message)
                            }
                            _ => HeyTurnOutcome::Failed(completed),
                        }
                    }
                    TurnStatus::Interrupted | TurnStatus::InProgress => {
                        HeyTurnOutcome::Failed(completed)
                    }
                });
            }
            _ => {}
        }
    }
}

#[ignore = "requires live GitHub Copilot credentials in ~/.codex-copilot"]
#[tokio::test]
async fn live_github_copilot_model_list_returns_models() -> Result<()> {
    let Some(source_home) = require_copilot_home() else {
        eprintln!(
            "skipping live_github_copilot_model_list_returns_models - ~/.codex-copilot/github-copilot-auth.json not found"
        );
        return Ok(());
    };

    let codex_home = TempDir::new()?;
    copy_live_copilot_auth(&source_home, &codex_home)?;
    write_live_copilot_config(&codex_home)?;

    let mut mcp = McpProcess::new(codex_home.path()).await?;
    timeout(DEFAULT_TIMEOUT, mcp.initialize()).await??;

    let account_request_id = mcp
        .send_get_account_request(GetAccountParams {
            refresh_token: false,
        })
        .await?;
    let account_response: JSONRPCResponse = timeout(
        DEFAULT_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(account_request_id)),
    )
    .await??;
    let account = to_response::<GetAccountResponse>(account_response)?;
    assert_eq!(account.account, Some(Account::GithubCopilot {}));
    assert!(!account.requires_openai_auth);

    let models = live_model_list(&mut mcp).await?;

    assert!(
        !models.data.is_empty(),
        "expected GitHub Copilot model/list to return at least one model"
    );
    assert!(
        models
            .data
            .iter()
            .all(|model| !model.model.trim().is_empty()),
        "expected every returned model slug to be non-empty"
    );
    Ok(())
}

#[ignore = "requires live GitHub Copilot credentials in ~/.codex-copilot"]
#[tokio::test]
async fn live_github_copilot_turn_start_hey_completes_without_401() -> Result<()> {
    let Some(source_home) = require_copilot_home() else {
        eprintln!(
            "skipping live_github_copilot_turn_start_hey_completes_without_401 - ~/.codex-copilot/github-copilot-auth.json not found"
        );
        return Ok(());
    };

    let codex_home = TempDir::new()?;
    copy_live_copilot_auth(&source_home, &codex_home)?;
    write_live_copilot_config(&codex_home)?;

    let mut mcp = McpProcess::new(codex_home.path()).await?;
    timeout(DEFAULT_TIMEOUT, mcp.initialize()).await??;

    let models = live_model_list(&mut mcp).await?;
    let model = models
        .data
        .iter()
        .find(|candidate| candidate.is_default)
        .or_else(|| models.data.first())
        .map(|candidate| candidate.model.clone())
        .expect("expected at least one GitHub Copilot model");

    let thread = start_thread_with_model(&mut mcp, model).await?.thread;

    let completed = match send_hey_turn_and_wait(&mut mcp, thread.id).await? {
        HeyTurnOutcome::Completed(completed) => completed,
        outcome => panic!("default GitHub Copilot model should complete the turn, got {outcome:?}"),
    };
    assert!(
        !completed.turn.id.trim().is_empty(),
        "expected completed turn id to be non-empty"
    );
    assert_eq!(
        completed.turn.status,
        codex_app_server_protocol::TurnStatus::Completed
    );

    Ok(())
}

#[ignore = "requires live GitHub Copilot credentials in ~/.codex-copilot"]
#[tokio::test]
async fn live_github_copilot_turn_start_hey_returns_assistant_text() -> Result<()> {
    let Some(source_home) = require_copilot_home() else {
        eprintln!(
            "skipping live_github_copilot_turn_start_hey_returns_assistant_text - ~/.codex-copilot/github-copilot-auth.json not found"
        );
        return Ok(());
    };

    let codex_home = TempDir::new()?;
    copy_live_copilot_auth(&source_home, &codex_home)?;
    write_live_copilot_config(&codex_home)?;

    let mut mcp = McpProcess::new(codex_home.path()).await?;
    timeout(DEFAULT_TIMEOUT, mcp.initialize()).await??;

    let models = live_model_list(&mut mcp).await?;
    let model = models
        .data
        .iter()
        .find(|candidate| candidate.is_default)
        .or_else(|| models.data.first())
        .map(|candidate| candidate.model.clone())
        .expect("expected at least one GitHub Copilot model");

    let thread = start_thread_with_model(&mut mcp, model).await?.thread;

    let turn_request_id = mcp
        .send_turn_start_request(TurnStartParams {
            thread_id: thread.id,
            input: vec![UserInput::Text {
                text: "hey".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    timeout(
        DEFAULT_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(turn_request_id)),
    )
    .await??;

    let mut assistant_text: Option<String> = None;
    loop {
        let message = timeout(DEFAULT_TIMEOUT, mcp.read_next_message()).await??;
        let JSONRPCMessage::Notification(notification) = message else {
            continue;
        };
        match notification.method.as_str() {
            "item/completed" => {
                let completed: ItemCompletedNotification = serde_json::from_value(
                    notification
                        .params
                        .expect("item/completed params must be present"),
                )?;
                if let ThreadItem::AgentMessage { text, .. } = completed.item
                    && !text.trim().is_empty()
                {
                    assistant_text = Some(text);
                }
            }
            "turn/completed" => {
                let completed: TurnCompletedNotification = serde_json::from_value(
                    notification
                        .params
                        .expect("turn/completed params must be present"),
                )?;
                assert_eq!(
                    completed.turn.status,
                    codex_app_server_protocol::TurnStatus::Completed
                );
                break;
            }
            _ => {}
        }
    }

    let assistant_text =
        assistant_text.expect("expected non-empty assistant text in item/completed");
    assert!(
        !assistant_text.trim().is_empty(),
        "expected assistant response text to be non-empty"
    );

    Ok(())
}

#[ignore = "requires live GitHub Copilot credentials in ~/.codex-copilot"]
#[tokio::test]
async fn live_github_copilot_model_matrix_reports_supported_vs_unsupported_models() -> Result<()> {
    let Some(source_home) = require_copilot_home() else {
        eprintln!(
            "skipping live_github_copilot_model_matrix_reports_supported_vs_unsupported_models - ~/.codex-copilot/github-copilot-auth.json not found"
        );
        return Ok(());
    };

    let codex_home = TempDir::new()?;
    copy_live_copilot_auth(&source_home, &codex_home)?;
    write_live_copilot_config(&codex_home)?;

    let mut mcp = McpProcess::new(codex_home.path()).await?;
    timeout(DEFAULT_TIMEOUT, mcp.initialize()).await??;

    let raw_models = live_raw_copilot_models(&codex_home).await?;
    let candidates = raw_models
        .data
        .iter()
        .filter(|model| MODEL_MATRIX_CANDIDATES.contains(&model.id.as_str()))
        .collect::<Vec<_>>();

    assert!(
        !candidates.is_empty(),
        "expected at least one matrix candidate to be available in raw Copilot /models"
    );

    let models = live_model_list(&mut mcp).await?;
    let listed = models
        .data
        .iter()
        .map(|model| model.model.as_str())
        .collect::<std::collections::BTreeSet<_>>();

    let mut supported = Vec::new();
    let mut unsupported = Vec::new();

    for model in candidates {
        let supports_responses = model
            .supported_endpoints
            .iter()
            .any(|endpoint| endpoint == "/responses");
        let is_listed = listed.contains(model.id.as_str());

        if supports_responses {
            assert!(
                is_listed,
                "expected model/list to include Responses-capable Copilot model {}",
                model.id
            );
        } else {
            assert!(
                !is_listed,
                "expected model/list to hide non-Responses Copilot model {}",
                model.id
            );
        }

        let thread = start_thread_with_model(&mut mcp, model.id.clone())
            .await?
            .thread;
        match send_hey_turn_and_wait(&mut mcp, thread.id).await? {
            HeyTurnOutcome::Completed(completed) => {
                eprintln!(
                    "[copilot matrix] supported: {} -> {:?}",
                    model.id, completed.turn.status
                );
                assert!(
                    supports_responses,
                    "expected only Responses-capable Copilot models to complete turns, but {} completed",
                    model.id
                );
                supported.push(model.id.clone());
            }
            HeyTurnOutcome::UnsupportedByResponsesApi(message) => {
                eprintln!("[copilot matrix] unsupported: {} -> {message}", model.id);
                assert!(
                    !supports_responses,
                    "expected Responses-capable Copilot model {} not to be rejected: {message}",
                    model.id
                );
                unsupported.push(model.id.clone());
            }
            HeyTurnOutcome::Failed(completed) => anyhow::bail!(
                "unexpected non-completed turn for model {}: status={:?} error={:?}",
                model.id,
                completed.turn.status,
                completed.turn.error
            ),
            HeyTurnOutcome::RpcError(error) => anyhow::bail!(
                "unexpected JSON-RPC error for model {}: {} ({})",
                model.id,
                error.error.message,
                error.error.code
            ),
        }
    }

    assert!(
        !supported.is_empty(),
        "expected at least one Copilot model in the matrix to support Responses API"
    );
    assert!(
        !unsupported.is_empty(),
        "expected at least one Copilot model in the matrix to be rejected by Responses API"
    );

    Ok(())
}
