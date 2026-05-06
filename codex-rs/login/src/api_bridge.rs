use codex_api::CoreAuthProvider;
use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::WireApi;

use crate::CodexAuth;
use crate::GEMINI_API_PROVIDER_ID;
use crate::read_gemini_api_key_candidates_from_env;

pub fn auth_provider_from_auth(
    auth: Option<CodexAuth>,
    provider: &ModelProviderInfo,
) -> codex_protocol::error::Result<CoreAuthProvider> {
    let (token_header_name, token_prefix) = match provider.wire_api {
        WireApi::Responses => (None, None),
        WireApi::Gemini if is_native_gemini_provider(provider) => {
            (Some("x-goog-api-key"), Some(""))
        }
        WireApi::Gemini => (None, None),
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
        if auth.is_api_key_auth() {
            let provider_matches = match auth.api_key_provider_id() {
                Some("openai") => !is_native_gemini_provider(provider),
                Some(GEMINI_API_PROVIDER_ID) => is_native_gemini_provider(provider),
                Some(_) => false,
                None => !is_native_gemini_provider(provider),
            };
            if provider_matches {
                let token = auth.get_token()?;
                return Ok(CoreAuthProvider {
                    token: Some(token),
                    account_id: auth.get_account_id(),
                    token_header_name,
                    token_prefix,
                });
            }
            return native_gemini_env_fallback(provider, token_header_name, token_prefix);
        }
        if is_native_gemini_provider(provider) {
            return native_gemini_env_fallback(provider, token_header_name, token_prefix);
        }

        let token = auth.get_token()?;
        return Ok(CoreAuthProvider {
            token: Some(token),
            account_id: auth.get_account_id(),
            token_header_name,
            token_prefix,
        });
    }

    native_gemini_env_fallback(provider, token_header_name, token_prefix)
}

fn is_native_gemini_provider(provider: &ModelProviderInfo) -> bool {
    provider.is_native_gemini_api()
}

fn native_gemini_env_fallback(
    provider: &ModelProviderInfo,
    token_header_name: Option<&'static str>,
    token_prefix: Option<&'static str>,
) -> codex_protocol::error::Result<CoreAuthProvider> {
    if is_native_gemini_provider(provider)
        && let Some((_, api_key)) = read_gemini_api_key_candidates_from_env().into_iter().next()
    {
        return Ok(CoreAuthProvider {
            token: Some(api_key),
            account_id: None,
            token_header_name,
            token_prefix,
        });
    }

    Ok(CoreAuthProvider {
        token: None,
        account_id: None,
        token_header_name,
        token_prefix,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn native_gemini_ignores_chatgpt_auth_tokens() {
        let provider = ModelProviderInfo::create_gemini_api_provider();
        let auth = Some(CodexAuth::create_dummy_chatgpt_auth_for_testing());

        let api_auth = auth_provider_from_auth(auth, &provider)
            .expect("native Gemini auth resolution should succeed");

        assert_eq!(api_auth.token, None);
        assert_eq!(api_auth.token_header_name, Some("x-goog-api-key"));
        assert_eq!(api_auth.token_prefix, Some(""));
    }

    #[test]
    fn native_gemini_uses_provider_api_key_when_present() {
        let provider = ModelProviderInfo::create_gemini_api_provider();
        let auth = Some(CodexAuth::from_api_key_for_provider(
            "gemini-key",
            Some(GEMINI_API_PROVIDER_ID.to_string()),
        ));

        let api_auth = auth_provider_from_auth(auth, &provider)
            .expect("native Gemini auth resolution should succeed");

        assert_eq!(api_auth.token.as_deref(), Some("gemini-key"));
        assert_eq!(api_auth.token_header_name, Some("x-goog-api-key"));
        assert_eq!(api_auth.token_prefix, Some(""));
    }
}
