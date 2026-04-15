use async_trait::async_trait;
use chrono::DateTime;
use chrono::Utc;
use reqwest::StatusCode;
use serde::Deserialize;
use serde::Serialize;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use crate::AuthManager;
use crate::ExternalAuth;
use crate::ExternalAuthRefreshContext;
use crate::ExternalAuthTokens;
use crate::copilot_storage::GitHubCopilotAuth;
use crate::copilot_storage::delete_github_copilot_auth;
use crate::copilot_storage::load_github_copilot_auth;
use crate::copilot_storage::save_github_copilot_auth;
use crate::default_client::build_reqwest_client;
use crate::token_data::parse_jwt_expiration;
use codex_app_server_protocol::AuthMode;
use codex_config::types::AuthCredentialsStoreMode;

pub const GITHUB_COPILOT_CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";
const GITHUB_DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const GITHUB_ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const GITHUB_COPILOT_TOKEN_URL: &str = "https://api.github.com/copilot_internal/v2/token";
const GITHUB_COPILOT_SCOPE: &str = "read:user";

#[derive(Clone, Debug)]
pub struct GitHubCopilotAuthProvider {
    codex_home: PathBuf,
    client: reqwest::Client,
}

#[derive(Debug, Clone)]
pub struct GitHubCopilotDeviceCode {
    pub verification_uri: String,
    pub user_code: String,
    pub interval: u64,
    pub expires_in: u64,
    device_code: String,
}

#[derive(Serialize)]
struct GitHubDeviceCodeRequest<'a> {
    client_id: &'a str,
    scope: &'a str,
}

#[derive(Debug, Deserialize)]
struct GitHubDeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    #[serde(default)]
    interval: Option<u64>,
}

#[derive(Serialize)]
struct GitHubAccessTokenRequest<'a> {
    client_id: &'a str,
    device_code: &'a str,
    grant_type: &'a str,
}

#[derive(Debug, Deserialize)]
struct GitHubAccessTokenResponse {
    access_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
    interval: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct GitHubCopilotTokenEnvelope {
    token: String,
}

#[derive(Debug, Deserialize)]
struct GitHubCopilotTokenResponse {
    endpoints: Option<GitHubCopilotEndpoints>,
    token: String,
    expires_at: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct GitHubCopilotEndpoints {
    api: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubCopilotSession {
    pub api_base_url: String,
    pub access_token: String,
    pub expires_at: Option<DateTime<Utc>>,
}

impl GitHubCopilotAuthProvider {
    pub fn new(codex_home: PathBuf) -> Self {
        Self {
            codex_home,
            client: build_reqwest_client(),
        }
    }

    async fn refresh_tokens(&self) -> std::io::Result<GitHubCopilotAuth> {
        let stored = load_github_copilot_auth(&self.codex_home)?
            .ok_or_else(|| std::io::Error::other("GitHub Copilot is not logged in."))?;
        let refreshed = exchange_copilot_token(&self.client, &stored.github_access_token).await?;
        let auth = GitHubCopilotAuth {
            api_base_url: Some(refreshed.api_base_url),
            github_access_token: stored.github_access_token,
            copilot_access_token: refreshed.access_token,
            copilot_token_expires_at: refreshed.expires_at,
        };
        save_github_copilot_auth(&self.codex_home, &auth)?;
        Ok(auth)
    }

    fn is_copilot_token_stale(auth: &GitHubCopilotAuth) -> bool {
        if let Some(expires_at) = auth.copilot_token_expires_at {
            return expires_at <= Utc::now() + chrono::Duration::minutes(1);
        }
        match parse_jwt_expiration(&auth.copilot_access_token) {
            Ok(Some(expires_at)) => expires_at <= Utc::now() + chrono::Duration::minutes(1),
            Ok(None) | Err(_) => false,
        }
    }
}

#[async_trait]
impl ExternalAuth for GitHubCopilotAuthProvider {
    fn auth_mode(&self) -> AuthMode {
        AuthMode::ApiKey
    }

    async fn resolve(&self) -> std::io::Result<Option<ExternalAuthTokens>> {
        let stored = match load_github_copilot_auth(&self.codex_home)? {
            Some(stored) => stored,
            None => return Ok(None),
        };

        let auth = if Self::is_copilot_token_stale(&stored) {
            self.refresh_tokens().await?
        } else {
            stored
        };

        Ok(Some(ExternalAuthTokens::access_token_only(
            auth.copilot_access_token,
        )))
    }

    async fn refresh(
        &self,
        _context: ExternalAuthRefreshContext,
    ) -> std::io::Result<ExternalAuthTokens> {
        let auth = self.refresh_tokens().await?;
        Ok(ExternalAuthTokens::access_token_only(
            auth.copilot_access_token,
        ))
    }
}

pub async fn request_github_copilot_device_code() -> std::io::Result<GitHubCopilotDeviceCode> {
    let client = build_reqwest_client();
    let response = client
        .post(GITHUB_DEVICE_CODE_URL)
        .header("Accept", "application/json")
        .form(&GitHubDeviceCodeRequest {
            client_id: GITHUB_COPILOT_CLIENT_ID,
            scope: GITHUB_COPILOT_SCOPE,
        })
        .send()
        .await
        .map_err(std::io::Error::other)?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(std::io::Error::other(format!(
            "GitHub Copilot device code request failed: {status}: {body}"
        )));
    }
    let response = response
        .json::<GitHubDeviceCodeResponse>()
        .await
        .map_err(std::io::Error::other)?;
    Ok(GitHubCopilotDeviceCode {
        verification_uri: response.verification_uri,
        user_code: response.user_code,
        interval: response.interval.unwrap_or(5),
        expires_in: response.expires_in,
        device_code: response.device_code,
    })
}

pub async fn run_github_copilot_device_code_login(codex_home: &Path) -> std::io::Result<()> {
    let client = build_reqwest_client();
    let device_code = request_github_copilot_device_code().await?;
    print_github_copilot_device_code_prompt(&device_code);
    let github_access_token = poll_github_access_token(&client, &device_code).await?;
    let copilot = exchange_copilot_token(&client, &github_access_token).await?;
    let auth = GitHubCopilotAuth {
        api_base_url: Some(copilot.api_base_url),
        github_access_token,
        copilot_access_token: copilot.access_token,
        copilot_token_expires_at: copilot.expires_at,
    };
    save_github_copilot_auth(codex_home, &auth)
}

pub async fn complete_github_copilot_device_code_login(
    codex_home: &Path,
    device_code: GitHubCopilotDeviceCode,
) -> std::io::Result<()> {
    let client = build_reqwest_client();
    let github_access_token = poll_github_access_token(&client, &device_code).await?;
    let copilot = exchange_copilot_token(&client, &github_access_token).await?;
    let auth = GitHubCopilotAuth {
        api_base_url: Some(copilot.api_base_url),
        github_access_token,
        copilot_access_token: copilot.access_token,
        copilot_token_expires_at: copilot.expires_at,
    };
    save_github_copilot_auth(codex_home, &auth)
}

pub fn logout_github_copilot(codex_home: &Path) -> std::io::Result<bool> {
    delete_github_copilot_auth(codex_home)
}

pub async fn load_github_copilot_session(
    codex_home: &Path,
) -> std::io::Result<Option<GitHubCopilotSession>> {
    let stored = match load_github_copilot_auth(codex_home)? {
        Some(stored) => stored,
        None => return Ok(None),
    };

    let refreshed =
        exchange_copilot_token(&build_reqwest_client(), &stored.github_access_token).await?;
    let auth = GitHubCopilotAuth {
        api_base_url: Some(refreshed.api_base_url.clone()),
        github_access_token: stored.github_access_token,
        copilot_access_token: refreshed.access_token.clone(),
        copilot_token_expires_at: refreshed.expires_at,
    };
    save_github_copilot_auth(codex_home, &auth)?;

    Ok(Some(refreshed))
}

impl AuthManager {
    pub fn native_github_copilot_bearer_only(codex_home: PathBuf) -> Arc<Self> {
        let manager = AuthManager::shared(
            codex_home.clone(),
            /*enable_codex_api_key_env*/ false,
            AuthCredentialsStoreMode::File,
        );
        manager.set_external_auth(Arc::new(GitHubCopilotAuthProvider::new(codex_home)));
        manager
    }
}

async fn poll_github_access_token(
    client: &reqwest::Client,
    device_code: &GitHubCopilotDeviceCode,
) -> std::io::Result<String> {
    let started_at = std::time::Instant::now();
    let max_wait = std::time::Duration::from_secs(device_code.expires_in);
    let mut interval = std::time::Duration::from_secs(device_code.interval.max(1));

    loop {
        let response = client
            .post(GITHUB_ACCESS_TOKEN_URL)
            .header("Accept", "application/json")
            .form(&GitHubAccessTokenRequest {
                client_id: GITHUB_COPILOT_CLIENT_ID,
                device_code: &device_code.device_code,
                grant_type: "urn:ietf:params:oauth:grant-type:device_code",
            })
            .send()
            .await
            .map_err(std::io::Error::other)?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(std::io::Error::other(format!(
                "GitHub access token request failed: {status}: {body}"
            )));
        }

        let response = response
            .json::<GitHubAccessTokenResponse>()
            .await
            .map_err(std::io::Error::other)?;
        if let Some(access_token) = response.access_token {
            return Ok(access_token);
        }

        match response.error.as_deref() {
            Some("authorization_pending") => {}
            Some("slow_down") => {
                interval += std::time::Duration::from_secs(response.interval.unwrap_or(5));
            }
            Some("expired_token") => {
                return Err(std::io::Error::other(
                    "GitHub Copilot device code expired before authorization completed.",
                ));
            }
            Some(error) => {
                let description = response.error_description.unwrap_or_default();
                return Err(std::io::Error::other(format!(
                    "GitHub access token request failed: {error}: {description}"
                )));
            }
            None => {
                return Err(std::io::Error::other(
                    "GitHub access token request returned neither a token nor an error.",
                ));
            }
        }

        if started_at.elapsed() >= max_wait {
            return Err(std::io::Error::other(
                "GitHub Copilot device code login timed out.",
            ));
        }

        tokio::time::sleep(interval.min(max_wait - started_at.elapsed())).await;
    }
}

async fn exchange_copilot_token(
    client: &reqwest::Client,
    github_access_token: &str,
) -> std::io::Result<GitHubCopilotSession> {
    let response = client
        .get(GITHUB_COPILOT_TOKEN_URL)
        .header("Accept", "application/json")
        .header("authorization", format!("token {github_access_token}"))
        .send()
        .await
        .map_err(std::io::Error::other)?;

    if response.status() == StatusCode::UNAUTHORIZED {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "GitHub Copilot token exchange returned 401. Sign in again.",
        ));
    }
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(std::io::Error::other(format!(
            "GitHub Copilot token exchange failed: {status}: {body}"
        )));
    }

    let body = response.text().await.map_err(std::io::Error::other)?;
    let token_response = serde_json::from_str::<GitHubCopilotTokenResponse>(&body)
        .or_else(|_| {
            serde_json::from_str::<GitHubCopilotTokenEnvelope>(&body).map(|token| {
                GitHubCopilotTokenResponse {
                    endpoints: None,
                    token: token.token,
                    expires_at: None,
                }
            })
        })
        .map_err(std::io::Error::other)?;

    let expires_at = token_response
        .expires_at
        .and_then(|expires_at| DateTime::<Utc>::from_timestamp(expires_at as i64, 0))
        .or_else(|| parse_jwt_expiration(&token_response.token).ok().flatten());

    let api_base_url = token_response
        .endpoints
        .and_then(|endpoints| endpoints.api)
        .unwrap_or_else(|| "https://api.githubcopilot.com".to_string());

    Ok(GitHubCopilotSession {
        api_base_url,
        access_token: token_response.token,
        expires_at,
    })
}

fn print_github_copilot_device_code_prompt(device_code: &GitHubCopilotDeviceCode) {
    println!(
        "\nSign in with GitHub Copilot:\n\n1. Open this URL in your browser\n   {}\n\n2. Enter this code\n   {}\n",
        device_code.verification_uri, device_code.user_code
    );
}
