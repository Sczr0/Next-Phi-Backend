use std::sync::Arc;

use axum::{
    Router,
    extract::Query,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Json, Redirect, Response},
    routing::{get, post},
};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Validation};
use moka::future::Cache;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use tokio::time::{Duration, sleep};
use uuid::Uuid;

use crate::{
    config::{AppConfig, OpenPlatformConfig},
    error::AppError,
    features::open_platform::storage,
    state::AppState,
};

const GITHUB_API_ACCEPT: &str = "application/vnd.github+json";
const GITHUB_TOKEN_ACCEPT: &str = "application/json";
const GITHUB_USER_AGENT: &str = "phi-backend-open-platform/0.1";

static OPEN_PLATFORM_AUTH_SERVICE: OnceCell<Arc<OpenPlatformAuthService>> = OnceCell::new();

#[derive(Clone)]
pub struct OpenPlatformAuthService {
    oauth_state_cache: Cache<String, bool>,
}

pub fn init_global(cfg: &OpenPlatformConfig) -> Result<(), AppError> {
    let service = OpenPlatformAuthService::new(cfg);
    OPEN_PLATFORM_AUTH_SERVICE
        .set(Arc::new(service))
        .map_err(|_| AppError::Internal("开放平台鉴权服务已初始化".into()))
}

fn global() -> Result<&'static Arc<OpenPlatformAuthService>, AppError> {
    OPEN_PLATFORM_AUTH_SERVICE
        .get()
        .ok_or_else(|| AppError::Internal("开放平台鉴权服务未初始化".into()))
}

impl OpenPlatformAuthService {
    fn new(cfg: &OpenPlatformConfig) -> Self {
        let ttl_secs = cfg.github.state_ttl_secs.max(60);
        let cache = Cache::builder()
            .max_capacity(10_000)
            .time_to_live(Duration::from_secs(ttl_secs))
            .build();
        Self {
            oauth_state_cache: cache,
        }
    }

    async fn issue_oauth_state(&self) -> String {
        let state = format!("ghs_{}", Uuid::new_v4().simple());
        self.oauth_state_cache.insert(state.clone(), true).await;
        state
    }

    async fn consume_oauth_state(&self, state: &str) -> bool {
        let exists = self.oauth_state_cache.get(state).await.unwrap_or(false);
        if exists {
            self.oauth_state_cache.invalidate(state).await;
        }
        exists
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DeveloperSessionClaims {
    sub: String,
    jti: String,
    github_user_id: String,
    github_login: String,
    iss: String,
    aud: String,
    iat: i64,
    exp: i64,
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
struct GithubTokenExchangeResponse {
    access_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GithubUserResponse {
    id: i64,
    login: String,
    email: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GithubEmailResponse {
    email: String,
    primary: bool,
    verified: bool,
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

fn map_reqwest_error(context: &'static str, err: reqwest::Error) -> AppError {
    if err.is_timeout() {
        AppError::Timeout(format!("{context}: {err}"))
    } else {
        AppError::Network(format!("{context}: {err}"))
    }
}

fn ensure_open_platform_enabled() -> Result<&'static OpenPlatformConfig, AppError> {
    let cfg = &AppConfig::global().open_platform;
    if !cfg.enabled {
        return Err(AppError::Validation("开放平台未启用".into()));
    }
    Ok(cfg)
}

fn resolve_session_secret(cfg: &OpenPlatformConfig) -> Result<String, AppError> {
    if !cfg.session.jwt_secret.trim().is_empty() {
        return Ok(cfg.session.jwt_secret.clone());
    }
    let from_env = std::env::var("APP_OPEN_PLATFORM_SESSION_JWT_SECRET").unwrap_or_default();
    if !from_env.trim().is_empty() {
        return Ok(from_env);
    }
    Err(AppError::Internal(
        "open_platform.session.jwt_secret 未配置（可通过 APP_OPEN_PLATFORM_SESSION_JWT_SECRET 设置）"
            .into(),
    ))
}

fn issue_developer_session_token(
    cfg: &OpenPlatformConfig,
    developer: &storage::DeveloperRecord,
) -> Result<String, AppError> {
    let now = chrono::Utc::now().timestamp();
    let claims = DeveloperSessionClaims {
        sub: developer.id.clone(),
        jti: Uuid::new_v4().to_string(),
        github_user_id: developer.github_user_id.clone(),
        github_login: developer.github_login.clone(),
        iss: cfg.session.jwt_issuer.clone(),
        aud: cfg.session.jwt_audience.clone(),
        iat: now,
        exp: now + cfg.session.ttl_secs.max(300) as i64,
    };
    let secret = resolve_session_secret(cfg)?;
    jsonwebtoken::encode(
        &jsonwebtoken::Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| AppError::Internal(format!("签发开发者会话令牌失败: {e}")))
}

fn decode_developer_session_token(
    cfg: &OpenPlatformConfig,
    token: &str,
) -> Result<DeveloperSessionClaims, AppError> {
    let secret = resolve_session_secret(cfg)?;
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_issuer(&[cfg.session.jwt_issuer.as_str()]);
    validation.set_audience(&[cfg.session.jwt_audience.as_str()]);
    let data = jsonwebtoken::decode::<DeveloperSessionClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map_err(|_| AppError::Auth("开发者会话无效或已过期".into()))?;
    Ok(data.claims)
}

fn read_cookie_value(headers: &HeaderMap, key: &str) -> Option<String> {
    let cookies = headers.get(header::COOKIE)?.to_str().ok()?;
    cookies.split(';').find_map(|part| {
        let mut iter = part.trim().splitn(2, '=');
        let name = iter.next()?.trim();
        let value = iter.next()?.trim();
        if name == key && !value.is_empty() {
            Some(value.to_string())
        } else {
            None
        }
    })
}

fn build_set_cookie_value(cfg: &OpenPlatformConfig, token: &str) -> String {
    let mut v = format!(
        "{}={}; Max-Age={}; Path=/; HttpOnly; SameSite=Lax",
        cfg.session.cookie_name, token, cfg.session.ttl_secs
    );
    if cfg.session.cookie_secure {
        v.push_str("; Secure");
    }
    v
}

fn build_clear_cookie_value(cfg: &OpenPlatformConfig) -> String {
    let mut v = format!(
        "{}=; Max-Age=0; Path=/; HttpOnly; SameSite=Lax",
        cfg.session.cookie_name
    );
    if cfg.session.cookie_secure {
        v.push_str("; Secure");
    }
    v
}

fn extract_developer_claims(
    headers: &HeaderMap,
    cfg: &OpenPlatformConfig,
) -> Result<DeveloperSessionClaims, AppError> {
    let token = read_cookie_value(headers, &cfg.session.cookie_name)
        .ok_or_else(|| AppError::Auth("缺少开发者会话".into()))?;
    decode_developer_session_token(cfg, &token)
}

/// 从开发者会话中解析当前开发者，并校验开发者仍存在于本地存储。
pub async fn require_developer(headers: &HeaderMap) -> Result<storage::DeveloperRecord, AppError> {
    let cfg = ensure_open_platform_enabled()?;
    let claims = extract_developer_claims(headers, cfg)?;
    let storage = storage::global()?;
    storage
        .get_developer_by_id(&claims.sub)
        .await?
        .ok_or_else(|| AppError::Auth("开发者会话已失效".into()))
}

async fn exchange_github_access_token(
    cfg: &OpenPlatformConfig,
    code: &str,
) -> Result<String, AppError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(cfg.github.http_timeout_secs.max(5)))
        .build()
        .map_err(|e| AppError::Internal(format!("初始化 GitHub OAuth 客户端失败: {e}")))?;

    let retry_count = cfg.github.http_retry_count.min(3);
    for attempt in 0..=retry_count {
        let req = client
            .post(&cfg.github.token_url)
            .header(header::ACCEPT.as_str(), GITHUB_TOKEN_ACCEPT)
            .header(header::USER_AGENT.as_str(), GITHUB_USER_AGENT)
            .form(&[
                ("client_id", cfg.github.client_id.as_str()),
                ("client_secret", cfg.github.client_secret.as_str()),
                ("code", code),
                ("redirect_uri", cfg.github.redirect_uri.as_str()),
            ]);

        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                let body = resp
                    .text()
                    .await
                    .map_err(|e| map_reqwest_error("读取 GitHub token 响应失败", e))?;

                if !status.is_success() {
                    if status.is_server_error() && attempt < retry_count {
                        sleep(Duration::from_millis(250 * (attempt as u64 + 1))).await;
                        continue;
                    }
                    return Err(AppError::Auth(format!(
                        "GitHub token 交换失败: HTTP {}",
                        status
                    )));
                }

                let parsed: GithubTokenExchangeResponse = serde_json::from_str(&body)
                    .map_err(|e| AppError::Network(format!("GitHub token 响应解析失败: {e}")))?;
                if let Some(err_code) = parsed.error {
                    let err_msg = parsed.error_description.unwrap_or_default();
                    return Err(AppError::Auth(format!(
                        "GitHub token 交换失败: {} {}",
                        err_code, err_msg
                    )));
                }
                let token = parsed
                    .access_token
                    .filter(|t| !t.trim().is_empty())
                    .ok_or_else(|| AppError::Auth("GitHub 未返回 access_token".into()))?;
                return Ok(token);
            }
            Err(e) => {
                if attempt < retry_count {
                    sleep(Duration::from_millis(250 * (attempt as u64 + 1))).await;
                    continue;
                }
                return Err(map_reqwest_error("请求 GitHub token 失败", e));
            }
        }
    }

    Err(AppError::Internal("GitHub token 交换失败".into()))
}

async fn fetch_github_user(
    cfg: &OpenPlatformConfig,
    access_token: &str,
) -> Result<GithubUserResponse, AppError> {
    let base = cfg.github.api_base_url.trim_end_matches('/');
    let url = format!("{base}/user");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(cfg.github.http_timeout_secs.max(5)))
        .build()
        .map_err(|e| AppError::Internal(format!("初始化 GitHub API 客户端失败: {e}")))?;

    let resp = client
        .get(url)
        .header(header::ACCEPT.as_str(), GITHUB_API_ACCEPT)
        .header(header::USER_AGENT.as_str(), GITHUB_USER_AGENT)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| map_reqwest_error("请求 GitHub /user 失败", e))?;

    let status = resp.status();
    if !status.is_success() {
        return Err(AppError::Auth(format!(
            "GitHub 用户信息查询失败: HTTP {}",
            status
        )));
    }
    resp.json::<GithubUserResponse>()
        .await
        .map_err(|e| AppError::Network(format!("GitHub /user 响应解析失败: {e}")))
}

async fn fetch_github_user_email(
    cfg: &OpenPlatformConfig,
    access_token: &str,
) -> Result<Option<String>, AppError> {
    let base = cfg.github.api_base_url.trim_end_matches('/');
    let url = format!("{base}/user/emails");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(cfg.github.http_timeout_secs.max(5)))
        .build()
        .map_err(|e| AppError::Internal(format!("初始化 GitHub API 客户端失败: {e}")))?;

    let resp = client
        .get(url)
        .header(header::ACCEPT.as_str(), GITHUB_API_ACCEPT)
        .header(header::USER_AGENT.as_str(), GITHUB_USER_AGENT)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| map_reqwest_error("请求 GitHub /user/emails 失败", e))?;
    if !resp.status().is_success() {
        return Ok(None);
    }
    let emails = resp
        .json::<Vec<GithubEmailResponse>>()
        .await
        .map_err(|e| AppError::Network(format!("GitHub /user/emails 响应解析失败: {e}")))?;

    if let Some(item) = emails.iter().find(|e| e.primary && e.verified) {
        return Ok(Some(item.email.clone()));
    }
    if let Some(item) = emails.iter().find(|e| e.verified) {
        return Ok(Some(item.email.clone()));
    }
    Ok(emails.first().map(|e| e.email.clone()))
}

#[utoipa::path(
    get,
    path = "/auth/github/login",
    summary = "发起 GitHub OAuth 登录",
    responses(
        (status = 307, description = "重定向到 GitHub 授权页"),
        (
            status = 500,
            description = "配置或服务初始化错误",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "OpenPlatformAuth"
)]
pub async fn get_github_login(
    Query(_query): Query<GithubLoginQuery>,
) -> Result<Response, AppError> {
    let cfg = ensure_open_platform_enabled()?;
    let service = global()?;
    let state = service.issue_oauth_state().await;

    let mut authorize_url = reqwest::Url::parse(&cfg.github.authorize_url)
        .map_err(|e| AppError::Internal(format!("GitHub authorize_url 非法: {e}")))?;
    authorize_url
        .query_pairs_mut()
        .append_pair("client_id", &cfg.github.client_id)
        .append_pair("redirect_uri", &cfg.github.redirect_uri)
        .append_pair("scope", &cfg.github.scope)
        .append_pair("state", &state)
        .append_pair("allow_signup", "true");

    Ok(Redirect::temporary(authorize_url.as_str()).into_response())
}

#[utoipa::path(
    get,
    path = "/auth/github/callback",
    summary = "GitHub OAuth 回调",
    responses(
        (status = 307, description = "登录成功并重定向控制台"),
        (
            status = 401,
            description = "state/code 无效或 GitHub 认证失败",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "服务端内部错误",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "OpenPlatformAuth"
)]
pub async fn get_github_callback(
    Query(query): Query<GithubCallbackQuery>,
) -> Result<Response, AppError> {
    let cfg = ensure_open_platform_enabled()?;
    let service = global()?;
    let storage = storage::global()?;

    if let Some(err) = query.error {
        return Err(AppError::Auth(format!(
            "GitHub OAuth 回调错误: {} {}",
            err,
            query.error_description.unwrap_or_default()
        )));
    }

    let state = query
        .state
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| AppError::Auth("缺少 OAuth state".into()))?;
    if !service.consume_oauth_state(state).await {
        return Err(AppError::Auth("OAuth state 无效或已过期".into()));
    }

    let code = query
        .code
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| AppError::Auth("缺少 OAuth code".into()))?;

    let access_token = exchange_github_access_token(cfg, code).await?;
    let user = fetch_github_user(cfg, &access_token).await?;
    let email = if user.email.is_some() {
        user.email
    } else {
        fetch_github_user_email(cfg, &access_token).await?
    };

    let now_ts = chrono::Utc::now().timestamp();
    let developer = storage
        .upsert_developer_by_github(&user.id.to_string(), &user.login, email.as_deref(), now_ts)
        .await?;
    let session_token = issue_developer_session_token(cfg, &developer)?;

    let mut res = Redirect::temporary(&cfg.github.post_login_redirect).into_response();
    let set_cookie = build_set_cookie_value(cfg, &session_token);
    res.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&set_cookie)
            .map_err(|e| AppError::Internal(format!("构造开发者会话 Cookie 失败: {e}")))?,
    );
    Ok(res)
}

#[utoipa::path(
    get,
    path = "/auth/me",
    summary = "获取当前开发者登录信息",
    responses(
        (status = 200, description = "当前开发者信息", body = DeveloperMeResponse),
        (
            status = 401,
            description = "缺少或无效开发者会话",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "OpenPlatformAuth"
)]
pub async fn get_me(
    headers: HeaderMap,
) -> Result<(StatusCode, Json<DeveloperMeResponse>), AppError> {
    let developer = require_developer(&headers).await?;

    Ok((
        StatusCode::OK,
        Json(DeveloperMeResponse {
            id: developer.id,
            github_user_id: developer.github_user_id,
            github_login: developer.github_login,
            email: developer.email,
            role: developer.role,
            status: developer.status,
        }),
    ))
}

#[utoipa::path(
    post,
    path = "/auth/logout",
    summary = "开发者退出登录",
    responses(
        (status = 200, description = "退出成功", body = LogoutResponse),
        (
            status = 500,
            description = "服务端内部错误",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        )
    ),
    tag = "OpenPlatformAuth"
)]
pub async fn post_logout() -> Result<Response, AppError> {
    let cfg = ensure_open_platform_enabled()?;
    let body = Json(LogoutResponse { ok: true });
    let mut resp = body.into_response();
    let clear_cookie = build_clear_cookie_value(cfg);
    resp.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&clear_cookie)
            .map_err(|e| AppError::Internal(format!("构造退出 Cookie 失败: {e}")))?,
    );
    Ok(resp)
}

pub fn create_open_platform_auth_router() -> Router<AppState> {
    Router::<AppState>::new()
        .route("/auth/github/login", get(get_github_login))
        .route("/auth/github/callback", get(get_github_callback))
        .route("/auth/me", get(get_me))
        .route("/auth/logout", post(post_logout))
}

#[cfg(test)]
mod tests {
    use super::{build_clear_cookie_value, build_set_cookie_value, read_cookie_value};
    use crate::config::OpenPlatformConfig;
    use axum::http::{HeaderMap, HeaderValue, header};

    fn test_cfg() -> OpenPlatformConfig {
        let mut cfg = OpenPlatformConfig::default();
        cfg.session.cookie_name = "op_session".into();
        cfg.session.cookie_secure = false;
        cfg
    }

    #[test]
    fn cookie_builder_sets_expected_flags() {
        let cfg = test_cfg();
        let set_cookie = build_set_cookie_value(&cfg, "abc");
        assert!(set_cookie.contains("op_session=abc"));
        assert!(set_cookie.contains("HttpOnly"));
        assert!(set_cookie.contains("SameSite=Lax"));

        let clear_cookie = build_clear_cookie_value(&cfg);
        assert!(clear_cookie.contains("Max-Age=0"));
    }

    #[test]
    fn cookie_reader_extracts_value() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("foo=1; op_session=token-123; bar=2"),
        );
        let got = read_cookie_value(&headers, "op_session");
        assert_eq!(got.as_deref(), Some("token-123"));
    }
}
