use codex_client::Request;
use http::HeaderMap;
use http::HeaderName;
use http::HeaderValue;

/// Provides bearer and account identity information for API requests.
///
/// Implementations should be cheap and non-blocking; any asynchronous
/// refresh or I/O should be handled by higher layers before requests
/// reach this interface.
pub trait AuthProvider: Send + Sync {
    fn bearer_token(&self) -> Option<String>;
    fn auth_header(&self) -> Option<(&'static str, String)> {
        self.bearer_token()
            .map(|token| ("authorization", format!("Bearer {token}")))
    }
    fn account_id(&self) -> Option<String> {
        None
    }
}

pub(crate) fn add_auth_headers_to_header_map<A: AuthProvider>(auth: &A, headers: &mut HeaderMap) {
    if let Some((header_name, header_value)) = auth.auth_header()
        && let Ok(value) = HeaderValue::from_str(&header_value)
    {
        let name = HeaderName::from_static(header_name);
        let _ = headers.insert(name, value);
    }
    if let Some(account_id) = auth.account_id()
        && let Ok(header) = HeaderValue::from_str(&account_id)
    {
        let _ = headers.insert("ChatGPT-Account-ID", header);
    }
}

pub(crate) fn add_auth_headers<A: AuthProvider>(auth: &A, mut req: Request) -> Request {
    add_auth_headers_to_header_map(auth, &mut req.headers);
    req
}
