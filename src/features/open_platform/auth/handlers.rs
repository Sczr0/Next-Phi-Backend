use crate::extract::ValidatedQuery;
use axum::{
    Json,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
};

use crate::{error::AppError, features::open_platform::storage};

use super::{
    github::{exchange_github_access_token, fetch_github_user, fetch_github_user_email},
    models::{DeveloperMeResponse, GithubCallbackQuery, GithubLoginQuery, LogoutResponse},
    service::global,
    session::{
        build_clear_cookie_value, build_set_cookie_value, ensure_open_platform_enabled,
        issue_developer_session_token, require_developer,
    },
};

#[utoipa::path(
    get,
    path = "/auth/github/login",
    summary = "发起 GitHub OAuth 登录",
    params(
        ("redirect" = Option<String>, Query, description = "保留字段：前端跳转意图（当前未使用）")
    ),
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
    ValidatedQuery(_query): ValidatedQuery<GithubLoginQuery>,
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
    description = "校验 OAuth state 与 code，换取 GitHub access token 并建立/复用开发者，随后签发 op_session 会话 Cookie 并 307 重定向到控制台。",
    params(
        ("code" = String, Query, description = "GitHub OAuth 授权码"),
        ("state" = String, Query, description = "OAuth state（来自 /auth/github/login，单次有效）"),
        ("error" = Option<String>, Query, description = "GitHub OAuth 错误标识（存在时直接返回 401）"),
        ("error_description" = Option<String>, Query, description = "GitHub OAuth 错误描述")
    ),
    responses(
        (status = 307, description = "登录成功并重定向控制台"),
        (
            status = 401,
            description = "state/code 无效或 GitHub 认证失败",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 502,
            description = "上游网络错误（GitHub API 调用失败）",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 504,
            description = "上游请求超时",
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
    ValidatedQuery(query): ValidatedQuery<GithubCallbackQuery>,
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
    security(("DeveloperCookie" = [])),
    responses(
        (status = 200, description = "当前开发者信息", body = DeveloperMeResponse),
        (
            status = 401,
            description = "缺少或无效开发者会话",
            body = crate::error::ProblemDetails,
            content_type = "application/problem+json"
        ),
        (
            status = 500,
            description = "开放平台存储未初始化",
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
