use std::sync::Arc;

use codex_model_provider_info::ModelProviderInfo;

use crate::AuthManager;

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
