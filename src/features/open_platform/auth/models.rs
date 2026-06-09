use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(super) struct DeveloperSessionClaims {
    pub sub: String,
    pub jti: String,
    pub github_user_id: String,
    pub github_login: String,
    pub iss: String,
    pub aud: String,
    pub iat: i64,
    pub exp: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubLoginQuery {
    /// 保留字段：前端可按需扩展跳转意图
    #[serde(default)]
    pub redirect: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct GithubTokenExchangeResponse {
    pub access_token: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct GithubUserResponse {
    pub id: i64,
    pub login: String,
    pub email: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct GithubEmailResponse {
    pub email: String,
    pub primary: bool,
    pub verified: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeveloperMeResponse {
    pub id: String,
    pub github_user_id: String,
    pub github_login: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    pub role: String,
    pub status: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogoutResponse {
    pub ok: bool,
}
