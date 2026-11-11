//! 委托授权 DTO。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct AuthorizeQuery {
    pub response_type: Option<String>,
    pub client_id: Option<String>,
    pub redirect_uri: Option<String>,
    pub state: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenForm {
    pub grant_type: Option<String>,
    pub code: Option<String>,
    pub redirect_uri: Option<String>,
    pub code_verifier: Option<String>,
    pub refresh_token: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RevokeForm {
    pub token: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub token_type_hint: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RequestView {
    pub request_id: String,
    pub client_id: String,
    pub redirect_uri: String,
    pub scope: String,
    pub state: Option<String>,
    pub expires_at: i64,
}

#[derive(Debug, Serialize)]
pub struct ApproveResponse {
    pub redirect_url: String,
}

#[derive(Debug, Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: &'static str,
    pub expires_in: u64,
    pub refresh_token: String,
    pub refresh_expires_in: u64,
    pub scope: String,
}

#[derive(Debug, Serialize)]
pub struct MeResponse {
    pub id: String,
    pub scopes: String,
}

#[derive(Debug, Serialize)]
pub struct OAuthError {
    pub error: &'static str,
    pub error_description: String,
}
