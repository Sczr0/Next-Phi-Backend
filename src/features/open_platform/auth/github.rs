use axum::http::header;
use tokio::time::{Duration, sleep};

use crate::{config::OpenPlatformConfig, error::AppError};

use super::models::{GithubEmailResponse, GithubTokenExchangeResponse, GithubUserResponse};

const GITHUB_API_ACCEPT: &str = "application/vnd.github+json";
const GITHUB_TOKEN_ACCEPT: &str = "application/json";
const GITHUB_USER_AGENT: &str = "phi-backend-open-platform/0.1";

fn map_reqwest_error(context: &'static str, err: &reqwest::Error) -> AppError {
    let sanitized = crate::error::sanitize_reqwest_error(err);
    if err.is_timeout() {
        AppError::Timeout(format!("{context}: {sanitized}"))
    } else {
        AppError::Network(format!("{context}: {sanitized}"))
    }
}

pub(super) async fn exchange_github_access_token(
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
                    .map_err(|e| map_reqwest_error("读取 GitHub token 响应失败", &e))?;

                if !status.is_success() {
                    if status.is_server_error() && attempt < retry_count {
                        sleep(Duration::from_millis(250 * (u64::from(attempt) + 1))).await;
                        continue;
                    }
                    return Err(AppError::Auth(format!(
                        "GitHub token 交换失败: HTTP {status}"
                    )));
                }

                let parsed: GithubTokenExchangeResponse = serde_json::from_str(&body)
                    .map_err(|e| AppError::Network(format!("GitHub token 响应解析失败: {e}")))?;
                if let Some(err_code) = parsed.error {
                    let err_msg = parsed.error_description.unwrap_or_default();
                    return Err(AppError::Auth(format!(
                        "GitHub token 交换失败: {err_code} {err_msg}"
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
                    sleep(Duration::from_millis(250 * (u64::from(attempt) + 1))).await;
                    continue;
                }
                return Err(map_reqwest_error("请求 GitHub token 失败", &e));
            }
        }
    }

    Err(AppError::Internal("GitHub token 交换失败".into()))
}

pub(super) async fn fetch_github_user(
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
        .map_err(|e| map_reqwest_error("请求 GitHub /user 失败", &e))?;

    let status = resp.status();
    if !status.is_success() {
        return Err(AppError::Auth(format!(
            "GitHub 用户信息查询失败: HTTP {status}"
        )));
    }
    resp.json::<GithubUserResponse>()
        .await
        .map_err(|e| AppError::Network(format!("GitHub /user 响应解析失败: {e}")))
}

pub(super) async fn fetch_github_user_email(
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
        .map_err(|e| map_reqwest_error("请求 GitHub /user/emails 失败", &e))?;
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
