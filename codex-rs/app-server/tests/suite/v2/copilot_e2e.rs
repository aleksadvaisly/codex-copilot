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
use codex_app_server_protocol::UserInput;
use tempfile::TempDir;
use tokio::time::timeout;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);

fn require_copilot_home() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let path = PathBuf::from(home).join(".codex-copilot");
    path.join("github-copilot-auth.json")
        .exists()
        .then_some(path)
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
    let auth_src = source_home.join("github-copilot-auth.json");
    let auth_dest = codex_home.path().join("github-copilot-auth.json");
    std::fs::copy(&auth_src, &auth_dest)
        .with_context(|| format!("copy {} -> {}", auth_src.display(), auth_dest.display()))?;
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
    let models = to_response::<ModelListResponse>(response)?;

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
    let auth_src = source_home.join("github-copilot-auth.json");
    let auth_dest = codex_home.path().join("github-copilot-auth.json");
    std::fs::copy(&auth_src, &auth_dest)
        .with_context(|| format!("copy {} -> {}", auth_src.display(), auth_dest.display()))?;
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

    let mut mcp = McpProcess::new(codex_home.path()).await?;
    timeout(DEFAULT_TIMEOUT, mcp.initialize()).await??;

    let model_list_request_id = mcp
        .send_list_models_request(ModelListParams {
            cursor: None,
            limit: Some(100),
            include_hidden: Some(true),
        })
        .await?;
    let model_list_response: JSONRPCResponse = timeout(
        DEFAULT_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(model_list_request_id)),
    )
    .await??;
    let models = to_response::<ModelListResponse>(model_list_response)?;
    let model = models
        .data
        .iter()
        .find(|candidate| candidate.is_default)
        .or_else(|| models.data.first())
        .map(|candidate| candidate.model.clone())
        .expect("expected at least one GitHub Copilot model");

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
    let thread = to_response::<ThreadStartResponse>(thread_response)?.thread;

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

    let completed = timeout(
        DEFAULT_TIMEOUT,
        mcp.read_stream_until_notification_message("turn/completed"),
    )
    .await??;
    let completed: TurnCompletedNotification = serde_json::from_value(
        completed
            .params
            .expect("turn/completed params must be present"),
    )?;
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
    let auth_src = source_home.join("github-copilot-auth.json");
    let auth_dest = codex_home.path().join("github-copilot-auth.json");
    std::fs::copy(&auth_src, &auth_dest)
        .with_context(|| format!("copy {} -> {}", auth_src.display(), auth_dest.display()))?;
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

    let mut mcp = McpProcess::new(codex_home.path()).await?;
    timeout(DEFAULT_TIMEOUT, mcp.initialize()).await??;

    let model_list_request_id = mcp
        .send_list_models_request(ModelListParams {
            cursor: None,
            limit: Some(100),
            include_hidden: Some(true),
        })
        .await?;
    let model_list_response: JSONRPCResponse = timeout(
        DEFAULT_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(model_list_request_id)),
    )
    .await??;
    let models = to_response::<ModelListResponse>(model_list_response)?;
    let model = models
        .data
        .iter()
        .find(|candidate| candidate.is_default)
        .or_else(|| models.data.first())
        .map(|candidate| candidate.model.clone())
        .expect("expected at least one GitHub Copilot model");

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
    let thread = to_response::<ThreadStartResponse>(thread_response)?.thread;

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
