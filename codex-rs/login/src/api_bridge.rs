use codex_api::CoreAuthProvider;
use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::WireApi;

use crate::CodexAuth;

pub fn auth_provider_from_auth(
    auth: Option<CodexAuth>,
    provider: &ModelProviderInfo,
) -> codex_protocol::error::Result<CoreAuthProvider> {
    let (token_header_name, token_prefix) = match provider.wire_api {
        WireApi::Responses | WireApi::Gemini => (None, None),
        WireApi::Anthropic => (Some("x-api-key"), Some("")),
    };

    if let Some(api_key) = provider.api_key()? {
        return Ok(CoreAuthProvider {
            token: Some(api_key),
            account_id: None,
            token_header_name,
            token_prefix,
        });
    }

    if let Some(token) = provider.experimental_bearer_token.clone() {
        return Ok(CoreAuthProvider {
            token: Some(token),
            account_id: None,
            token_header_name,
            token_prefix,
        });
    }

    if let Some(auth) = auth {
        let token = auth.get_token()?;
        Ok(CoreAuthProvider {
            token: Some(token),
            account_id: auth.get_account_id(),
            token_header_name,
            token_prefix,
        })
    } else {
        Ok(CoreAuthProvider {
            token: None,
            account_id: None,
            token_header_name,
            token_prefix,
        })
    }
}
