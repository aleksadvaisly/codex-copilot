use std::sync::Arc;

use codex_api::CoreAuthProvider;
use codex_api::Provider as ApiProvider;
use codex_model_provider_info::ModelProviderInfo;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;

use crate::AuthManager;
use crate::CodexAuth;
use crate::auth_provider_from_auth;
use crate::copilot_storage::load_github_copilot_auth;
use crate::load_github_copilot_session;

pub struct ResolvedProviderApiAccess {
    pub auth: Option<CodexAuth>,
    pub api_provider: ApiProvider,
    pub api_auth: CoreAuthProvider,
}

fn is_github_copilot_provider(provider: &ModelProviderInfo) -> bool {
    provider.name == "GitHub Copilot"
        || provider.base_url.as_deref() == Some("https://api.githubcopilot.com")
}

/// Returns the provider-scoped auth manager when this provider uses command-backed auth.
///
/// Providers without custom auth continue using the caller-supplied base manager.
pub fn auth_manager_for_provider(
    auth_manager: Option<Arc<AuthManager>>,
    provider: &ModelProviderInfo,
) -> Option<Arc<AuthManager>> {
    if is_github_copilot_provider(provider) {
        return auth_manager
            .map(|manager| AuthManager::native_github_copilot_bearer_only(manager.codex_home()));
    }
    match provider.auth.clone() {
        Some(config) => Some(AuthManager::external_bearer_only(config)),
        None => auth_manager,
    }
}

/// Returns an auth manager for request paths that always require authentication.
///
/// Providers with command-backed auth get a bearer-only manager; otherwise the caller's manager
/// is reused unchanged.
pub fn required_auth_manager_for_provider(
    auth_manager: Arc<AuthManager>,
    provider: &ModelProviderInfo,
) -> Arc<AuthManager> {
    if is_github_copilot_provider(provider) {
        return AuthManager::native_github_copilot_bearer_only(auth_manager.codex_home());
    }
    match provider.auth.clone() {
        Some(config) => AuthManager::external_bearer_only(config),
        None => auth_manager,
    }
}

pub async fn resolve_provider_api_access(
    auth_manager: Option<Arc<AuthManager>>,
    provider: &ModelProviderInfo,
) -> CodexResult<ResolvedProviderApiAccess> {
    let auth = match auth_manager.as_ref() {
        Some(manager) => manager.auth().await,
        None => None,
    };
    let mut api_provider = provider.to_api_provider(auth.as_ref().map(CodexAuth::auth_mode))?;
    let mut api_auth = auth_provider_from_auth(auth.clone(), provider)?;

    if is_github_copilot_provider(provider)
        && let Some(codex_home) = auth_manager.as_ref().map(|manager| manager.codex_home())
    {
        let stored = load_github_copilot_auth(&codex_home).map_err(CodexErr::Io)?;
        if let Some(api_base_url) = stored
            .as_ref()
            .and_then(|auth| auth.api_base_url.as_ref())
            .filter(|value| !value.trim().is_empty())
        {
            api_provider.base_url = api_base_url.clone();
        } else if let Some(session) = load_github_copilot_session(&codex_home)
            .await
            .map_err(CodexErr::Io)?
        {
            api_provider.base_url = session.api_base_url;
            api_auth = CoreAuthProvider {
                token: Some(session.access_token),
                account_id: None,
                token_header_name: None,
                token_prefix: None,
            };
        }
    }

    Ok(ResolvedProviderApiAccess {
        auth,
        api_provider,
        api_auth,
    })
}
