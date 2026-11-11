//! 单 client 的简化 OAuth2 授权端点。

use axum::Extension;
use axum::extract::{Form, Path, Query};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Redirect, Response};

use crate::error::AppError;
use crate::features::auth::bearer::{BearerAuthState, decode_embedded_auth_with_claims};

use super::crypto::{
    constant_time_eq, new_id, new_token, open_credential, scopes_contain, seal_credential,
    sha256_hex, verify_pkce,
};
use super::models::{
    ApproveResponse, AuthorizeQuery, MeResponse, OAuthError, RequestView, RevokeForm, TokenForm,
    TokenResponse,
};
use super::{config, storage};

pub const SAVE_SCOPE: &str = "phigros.query.save.read";
const CODE_TTL_SECS: i64 = 60;
const REQUEST_TTL_SECS: i64 = 600;

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn oauth_error(
    status: StatusCode,
    error: &'static str,
    description: impl Into<String>,
) -> Response {
    (
        status,
        Json(OAuthError {
            error,
            error_description: description.into(),
        }),
    )
        .into_response()
}

fn internal(error: AppError) -> Response {
    tracing::error!(error = %error, "delegation internal error");
    oauth_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "server_error",
        "internal error",
    )
}

fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn with_query(base: &str, pairs: &[(&str, &str)]) -> String {
    let separator = if base.contains('?') { '&' } else { '?' };
    let mut out = format!("{base}{separator}");
    for (index, (key, value)) in pairs.iter().enumerate() {
        if index > 0 {
            out.push('&');
        }
        out.push_str(&percent_encode(key));
        out.push('=');
        out.push_str(&percent_encode(value));
    }
    out
}

fn redirect_error(redirect_uri: &str, state: Option<&str>, error: &str) -> Response {
    let mut pairs = vec![("error", error)];
    if let Some(state) = state {
        pairs.push(("state", state));
    }
    Redirect::temporary(&with_query(redirect_uri, &pairs)).into_response()
}

fn valid_pkce_challenge(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn parse_scope(requested: Option<&str>, allowed: &[String]) -> Result<String, &'static str> {
    let raw: Vec<&str> = requested.unwrap_or("").split_whitespace().collect();
    if raw.is_empty() {
        return Ok(allowed.join(" "));
    }
    if raw
        .iter()
        .any(|scope| !allowed.iter().any(|item| item == scope))
    {
        return Err("invalid_scope");
    }
    Ok(raw.join(" "))
}

fn bearer_of(
    bearer: &BearerAuthState,
) -> Result<(&str, &crate::features::auth::bearer::SessionClaims), AppError> {
    match bearer {
        BearerAuthState::Valid(context) => Ok((context.token.as_str(), &context.claims)),
        BearerAuthState::Invalid(message) => Err(AppError::Auth(message.clone())),
        BearerAuthState::Absent => Err(AppError::Auth("需要登录后同意".into())),
    }
}

pub async fn authorize(Query(query): Query<AuthorizeQuery>) -> Response {
    let cfg = config();
    let Some(client_id) = query.client_id.as_deref() else {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "client_id is required",
        );
    };
    let Some(redirect_uri) = query.redirect_uri.as_deref() else {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "redirect_uri is required",
        );
    };
    if client_id != cfg.client_id {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_client", "unknown client");
    }
    if redirect_uri != cfg.redirect_uri {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "redirect_uri is not registered",
        );
    }
    if query.response_type.as_deref() != Some("code") {
        return redirect_error(
            redirect_uri,
            query.state.as_deref(),
            "unsupported_response_type",
        );
    }
    let code_challenge = query.code_challenge.as_deref().unwrap_or_default();
    if !valid_pkce_challenge(code_challenge)
        || query.code_challenge_method.as_deref() != Some("S256")
    {
        return redirect_error(redirect_uri, query.state.as_deref(), "invalid_request");
    }
    let scope = match parse_scope(query.scope.as_deref(), &cfg.scopes) {
        Ok(scope) => scope,
        Err(error) => return redirect_error(redirect_uri, query.state.as_deref(), error),
    };

    let request_id = new_id();
    let created = now();
    if let Err(error) = storage()
        .insert_request(
            &request_id,
            client_id,
            redirect_uri,
            query.state.as_deref(),
            code_challenge,
            &scope,
            created,
            created + REQUEST_TTL_SECS,
        )
        .await
    {
        return internal(error);
    }

    let consent = with_query(&cfg.consent_url, &[("request_id", request_id.as_str())]);
    Redirect::temporary(&consent).into_response()
}

pub async fn request(Path(id): Path<String>) -> Response {
    let Ok(Some(request)) = storage().request(&id).await else {
        return oauth_error(
            StatusCode::NOT_FOUND,
            "not_found",
            "authorization request not found",
        );
    };
    if request.expires_at <= now() {
        return oauth_error(StatusCode::GONE, "expired", "authorization request expired");
    }
    Json(RequestView {
        request_id: request.id,
        client_id: request.client_id,
        redirect_uri: request.redirect_uri,
        scope: request.scope,
        state: request.state,
        expires_at: request.expires_at,
    })
    .into_response()
}

pub async fn approve(
    Path(id): Path<String>,
    Extension(bearer): Extension<BearerAuthState>,
) -> Response {
    let cfg = config();
    let (token, claims) = match bearer_of(&bearer) {
        Ok(value) => value,
        Err(error) => {
            return oauth_error(
                StatusCode::UNAUTHORIZED,
                "login_required",
                error.to_string(),
            );
        }
    };
    let Ok(Some(request)) = storage().request(&id).await else {
        return oauth_error(
            StatusCode::NOT_FOUND,
            "not_found",
            "authorization request not found",
        );
    };
    if request.expires_at <= now() {
        return oauth_error(StatusCode::GONE, "expired", "authorization request expired");
    }
    let auth = match decode_embedded_auth_with_claims(token, claims) {
        Ok(auth) => auth,
        Err(error) => {
            return oauth_error(
                StatusCode::UNAUTHORIZED,
                "login_required",
                error.to_string(),
            );
        }
    };
    let Some(session_token) = auth.session_token.clone() else {
        return oauth_error(
            StatusCode::UNAUTHORIZED,
            "needs_reauth",
            "a LeanCloud session token is required",
        );
    };
    let subject = claims.sub.clone();

    let now = now();
    match storage().consume_request(&id, now).await {
        Ok(true) => {}
        Ok(false) => {
            return oauth_error(
                StatusCode::CONFLICT,
                "invalid_request",
                "request already used",
            );
        }
        Err(error) => return internal(error),
    }

    let delegation_id = new_id();
    let sealed = match seal_credential(&cfg.credential_key, &delegation_id, session_token.as_str())
    {
        Ok(sealed) => sealed,
        Err(error) => return internal(error),
    };
    if let Err(error) = storage()
        .insert_delegation(
            &delegation_id,
            &request.client_id,
            &subject,
            &request.scope,
            &sealed,
            i64::from(cfg.credential_key_version),
            now,
            now + i64::try_from(cfg.delegation_ttl_secs).unwrap_or(i64::MAX),
        )
        .await
    {
        return internal(error);
    }

    let code = new_token("dc_");
    if let Err(error) = storage()
        .insert_code(
            &sha256_hex(&code),
            &delegation_id,
            &request.client_id,
            &request.redirect_uri,
            &request.code_challenge,
            &request.scope,
            now,
            now + CODE_TTL_SECS,
        )
        .await
    {
        return internal(error);
    }

    let mut pairs = vec![("code", code.as_str())];
    if let Some(state) = request.state.as_deref() {
        pairs.push(("state", state));
    }
    Json(ApproveResponse {
        redirect_url: with_query(&request.redirect_uri, &pairs),
    })
    .into_response()
}

pub async fn deny(Path(id): Path<String>) -> Response {
    let Ok(Some(request)) = storage().request(&id).await else {
        return oauth_error(
            StatusCode::NOT_FOUND,
            "not_found",
            "authorization request not found",
        );
    };
    if request.expires_at <= now() {
        return oauth_error(StatusCode::GONE, "expired", "authorization request expired");
    }
    if let Err(error) = storage().consume_request(&id, now()).await {
        return internal(error);
    }
    redirect_error(
        &request.redirect_uri,
        request.state.as_deref(),
        "access_denied",
    )
}

fn valid_client(form: &TokenForm) -> bool {
    let cfg = config();
    let Some(client_id) = form.client_id.as_deref() else {
        return false;
    };
    let Some(secret) = form.client_secret.as_deref() else {
        return false;
    };
    client_id == cfg.client_id && constant_time_eq(secret.as_bytes(), cfg.client_secret.as_bytes())
}

async fn issue_tokens(
    delegation_id: &str,
    scopes: &str,
    family_id: &str,
) -> Result<TokenResponse, AppError> {
    let cfg = config();
    let now = now();
    let access = new_token("dqa_");
    let refresh = new_token("dqr_");
    let access_expires = now + i64::try_from(cfg.access_ttl_secs).unwrap_or(i64::MAX);
    let refresh_expires = now + i64::try_from(cfg.refresh_ttl_secs).unwrap_or(i64::MAX);
    storage()
        .insert_access(
            &sha256_hex(&access),
            delegation_id,
            scopes,
            now,
            access_expires,
        )
        .await?;
    storage()
        .insert_refresh(
            &sha256_hex(&refresh),
            family_id,
            delegation_id,
            now,
            refresh_expires,
        )
        .await?;
    Ok(TokenResponse {
        access_token: access,
        token_type: "Bearer",
        expires_in: cfg.access_ttl_secs,
        refresh_token: refresh,
        refresh_expires_in: cfg.refresh_ttl_secs,
        scope: scopes.to_string(),
    })
}

pub async fn token(Form(form): Form<TokenForm>) -> Response {
    if !valid_client(&form) {
        return oauth_error(
            StatusCode::UNAUTHORIZED,
            "invalid_client",
            "client authentication failed",
        );
    }
    let now = now();
    match form.grant_type.as_deref() {
        Some("authorization_code") => {
            let (Some(code), Some(redirect_uri), Some(verifier)) = (
                form.code.as_deref(),
                form.redirect_uri.as_deref(),
                form.code_verifier.as_deref(),
            ) else {
                return oauth_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "missing code fields",
                );
            };
            let row = match storage().consume_code(&sha256_hex(code), now).await {
                Ok(Some(row)) => row,
                Ok(None) => {
                    return oauth_error(StatusCode::BAD_REQUEST, "invalid_grant", "invalid code");
                }
                Err(error) => return internal(error),
            };
            let cfg = config();
            if row.client_id != cfg.client_id
                || row.redirect_uri != redirect_uri
                || !verify_pkce(verifier, &row.code_challenge)
            {
                return oauth_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    "code does not match",
                );
            }
            match storage().delegation(&row.delegation_id).await {
                Ok(Some(delegation))
                    if delegation.revoked_at.is_none() && delegation.expires_at > now => {}
                Ok(_) => {
                    return oauth_error(
                        StatusCode::BAD_REQUEST,
                        "invalid_grant",
                        "delegation expired",
                    );
                }
                Err(error) => return internal(error),
            }
            match issue_tokens(&row.delegation_id, &row.scopes, &new_id()).await {
                Ok(response) => Json(response).into_response(),
                Err(error) => internal(error),
            }
        }
        Some("refresh_token") => {
            let Some(refresh_token) = form.refresh_token.as_deref() else {
                return oauth_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "missing refresh_token",
                );
            };
            let hash = sha256_hex(refresh_token);
            let row = match storage().refresh(&hash).await {
                Ok(Some(row)) => row,
                Ok(None) => {
                    return oauth_error(
                        StatusCode::BAD_REQUEST,
                        "invalid_grant",
                        "invalid refresh token",
                    );
                }
                Err(error) => return internal(error),
            };
            if row.consumed_at.is_some() {
                if let Err(error) = storage().revoke_family(&row.family_id, now).await {
                    return internal(error);
                }
                return oauth_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    "refresh token reused",
                );
            }
            if row.revoked_at.is_some() || row.expires_at <= now {
                return oauth_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    "refresh token expired",
                );
            }
            match storage().consume_refresh(&hash, now).await {
                Ok(Some(_)) => {}
                Ok(None) => {
                    let _ = storage().revoke_family(&row.family_id, now).await;
                    return oauth_error(
                        StatusCode::BAD_REQUEST,
                        "invalid_grant",
                        "refresh token reused",
                    );
                }
                Err(error) => return internal(error),
            }
            let delegation = match storage().delegation(&row.delegation_id).await {
                Ok(Some(delegation))
                    if delegation.revoked_at.is_none() && delegation.expires_at > now =>
                {
                    delegation
                }
                Ok(_) => {
                    return oauth_error(
                        StatusCode::BAD_REQUEST,
                        "invalid_grant",
                        "delegation expired",
                    );
                }
                Err(error) => return internal(error),
            };
            match issue_tokens(&delegation.id, &delegation.scopes, &row.family_id).await {
                Ok(response) => Json(response).into_response(),
                Err(error) => internal(error),
            }
        }
        _ => oauth_error(
            StatusCode::BAD_REQUEST,
            "unsupported_grant_type",
            "unsupported grant_type",
        ),
    }
}

pub async fn revoke(Form(form): Form<RevokeForm>) -> Response {
    if !valid_client(&TokenForm {
        grant_type: None,
        code: None,
        redirect_uri: None,
        code_verifier: None,
        refresh_token: None,
        client_id: form.client_id,
        client_secret: form.client_secret,
    }) {
        return oauth_error(
            StatusCode::UNAUTHORIZED,
            "invalid_client",
            "client authentication failed",
        );
    }
    let Some(token) = form.token.as_deref() else {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "token is required",
        );
    };
    let now = now();
    if token.starts_with("dqa_") {
        let hash = sha256_hex(token);
        match storage().authenticate_access(&hash, now).await {
            Ok(Some(context)) => {
                if let Err(error) = storage()
                    .revoke_delegation(&context.delegation_id, now)
                    .await
                {
                    return internal(error);
                }
            }
            Ok(None) => {
                if let Err(error) = storage().revoke_access(&hash, now).await {
                    return internal(error);
                }
            }
            Err(error) => return internal(error),
        }
    } else if token.starts_with("dqr_") {
        let hash = sha256_hex(token);
        match storage().find_refresh_delegation(&hash).await {
            Ok(Some(delegation_id)) => {
                if let Err(error) = storage().revoke_delegation(&delegation_id, now).await {
                    return internal(error);
                }
            }
            Ok(None) => {}
            Err(error) => return internal(error),
        }
    }
    StatusCode::OK.into_response()
}

pub async fn me(headers: HeaderMap) -> Response {
    let Some(token) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|value| value.starts_with("dqa_"))
    else {
        return oauth_error(
            StatusCode::UNAUTHORIZED,
            "invalid_token",
            "missing delegation token",
        );
    };
    match storage()
        .authenticate_access(&sha256_hex(token), now())
        .await
    {
        Ok(Some(context)) => Json(MeResponse {
            id: context.subject,
            scopes: context.scopes,
        })
        .into_response(),
        Ok(None) => oauth_error(
            StatusCode::UNAUTHORIZED,
            "invalid_token",
            "token is not active",
        ),
        Err(error) => internal(error),
    }
}

/// `/save` 等资源端使用：返回委托绑定的 LeanCloud sessionToken。
///
/// 非 `dqa_` 前缀返回 `None`，让调用方回落到原有 session Bearer 逻辑。
pub async fn authenticate_bearer(
    headers: &HeaderMap,
    required_scope: &str,
) -> Result<Option<String>, AppError> {
    let Some(token) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
    else {
        return Ok(None);
    };
    if !token.starts_with("dqa_") {
        return Ok(None);
    }
    let Some(context) = storage()
        .authenticate_access(&sha256_hex(token), now())
        .await?
    else {
        return Err(AppError::Auth("delegation token is not active".into()));
    };
    if !scopes_contain(&context.scopes, required_scope) {
        return Err(AppError::Auth(format!(
            "delegation token lacks scope {required_scope}"
        )));
    }
    let credential = open_credential(
        &config().credential_key,
        &context.delegation_id,
        &context.credential_ciphertext,
    )?;
    Ok(Some(credential))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_parser_rejects_unknown_scope() {
        let allowed = vec!["a".to_string(), "b".to_string()];
        assert_eq!(parse_scope(Some("a b"), &allowed), Ok("a b".to_string()));
        assert_eq!(parse_scope(Some("a c"), &allowed), Err("invalid_scope"));
        assert_eq!(parse_scope(None, &allowed), Ok("a b".to_string()));
    }

    #[test]
    fn pkce_shape_accepts_43_unreserved() {
        assert!(valid_pkce_challenge(&"a".repeat(43)));
        assert!(!valid_pkce_challenge(&"a".repeat(42)));
        assert!(!valid_pkce_challenge(&"a".repeat(43).replace('a', "!")));
    }
}
