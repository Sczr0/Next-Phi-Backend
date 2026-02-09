use std::sync::{Arc, Once};

use axum::Json;
use axum::body::{Bytes, to_bytes};
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use moka::future::Cache;
use tokio::sync::Semaphore;
use uuid::Uuid;

use phi_backend::config::{AppConfig, TapTapConfig, TapTapMultiConfig, TapTapVersion};
use phi_backend::features::auth::client::TapTapClient;
use phi_backend::features::auth::handler::{
    SessionExchangeRequest, SessionLogoutRequest, SessionLogoutScope, post_session_exchange,
    post_session_logout,
};
use phi_backend::features::auth::qrcode_service::QrCodeService;
use phi_backend::features::save::models::UnifiedSaveRequest;
use phi_backend::features::song::models::SongCatalog;
use phi_backend::features::stats::storage::StatsStorage;
use phi_backend::state::AppState;

fn init_test_config() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        unsafe {
            std::env::set_var("APP_STATS_USER_HASH_SALT", "test-user-hash-salt");
            std::env::set_var("APP_SESSION_JWT_SECRET", "test-jwt-secret");
            std::env::set_var("APP_SESSION_EXCHANGE_SHARED_SECRET", "test-exchange-secret");
        }
        let _ = AppConfig::init_global();
    });
}

fn dummy_taptap_config(app_id: &str, app_key: &str) -> TapTapConfig {
    TapTapConfig {
        device_code_endpoint: "http://example.invalid/device/code".to_string(),
        token_endpoint: "http://example.invalid/token".to_string(),
        user_info_endpoint: "http://example.invalid/userinfo".to_string(),
        leancloud_base_url: "http://example.invalid/leancloud".to_string(),
        leancloud_app_id: app_id.to_string(),
        leancloud_app_key: app_key.to_string(),
    }
}

fn make_state() -> AppState {
    let taptap_cfg = TapTapMultiConfig {
        cn: dummy_taptap_config("cn-app-id", "cn-app-key"),
        global: dummy_taptap_config("global-app-id", "global-app-key"),
        default_version: TapTapVersion::CN,
    };
    let taptap_client = Arc::new(TapTapClient::new(&taptap_cfg).expect("TapTapClient::new"));
    let qrcode_service = Arc::new(QrCodeService::new());

    let bn_image_cache: Cache<String, Bytes> = Cache::builder().max_capacity(16).build();
    let song_image_cache: Cache<String, Bytes> = Cache::builder().max_capacity(16).build();

    AppState {
        chart_constants: Arc::new(Default::default()),
        song_catalog: Arc::new(SongCatalog::default()),
        taptap_client,
        qrcode_service,
        stats: None,
        stats_storage: None,
        render_semaphore: Arc::new(Semaphore::new(1)),
        bn_image_cache,
        song_image_cache,
    }
}

async fn make_state_with_storage() -> AppState {
    let mut state = make_state();
    let db_path = format!("./resources/test_auth_session_{}.db", Uuid::new_v4());
    let storage = StatsStorage::connect_sqlite(&db_path, true)
        .await
        .expect("connect sqlite");
    storage.init_schema().await.expect("init schema");
    state.stats_storage = Some(Arc::new(storage));
    state
}

fn make_exchange_request() -> SessionExchangeRequest {
    SessionExchangeRequest {
        auth: UnifiedSaveRequest {
            session_token: Some("r:test-session-token".to_string()),
            external_credentials: None,
            taptap_version: None,
        },
    }
}

fn make_exchange_headers(secret: &str) -> axum::http::HeaderMap {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        "x-exchange-secret",
        axum::http::HeaderValue::from_str(secret).expect("header value"),
    );
    headers
}

async fn exchange_token(secret: &str) -> String {
    let state = make_state_with_storage().await;
    let (_, Json(resp)) =
        post_session_exchange(
            State(state),
            make_exchange_headers(secret),
            Json(make_exchange_request()),
        )
            .await
            .expect("exchange success");
    resp.access_token
}

fn decode_claims(token: &str) -> serde_json::Value {
    let cfg = &AppConfig::global().session;
    let jwt_secret = if !cfg.jwt_secret.trim().is_empty() {
        cfg.jwt_secret.clone()
    } else {
        std::env::var("APP_SESSION_JWT_SECRET").expect("APP_SESSION_JWT_SECRET")
    };
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
    validation.validate_exp = true;
    validation.set_issuer(&[cfg.jwt_issuer.as_str()]);
    validation.set_audience(&[cfg.jwt_audience.as_str()]);
    let data = jsonwebtoken::decode::<serde_json::Value>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(jwt_secret.as_bytes()),
        &validation,
    )
    .expect("decode token claims");
    data.claims
}

#[tokio::test]
async fn qrcode_status_missing_is_expired_and_no_store() {
    let state = make_state();

    let resp = phi_backend::features::auth::handler::get_qrcode_status(
        State(state),
        Path("missing".to_string()),
    )
    .await
    .expect("handler ok");

    assert_eq!(resp.status(), StatusCode::OK);

    let cache_control = resp
        .headers()
        .get(header::CACHE_CONTROL)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(cache_control, "no-store");

    let bytes = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("read body");
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");
    assert_eq!(v["status"], "Expired");
}

#[tokio::test]
async fn qrcode_status_expires_in_zero_becomes_expired() {
    let state = make_state();
    let qr_id = "test-qr-id".to_string();

    state
        .qrcode_service
        .set_pending(
            qr_id.clone(),
            "device_code".to_string(),
            "device_id".to_string(),
            5,
            Some(0),
            None,
        )
        .await;

    let resp = phi_backend::features::auth::handler::get_qrcode_status(State(state), Path(qr_id))
        .await
        .expect("handler ok");

    let bytes = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("read body");
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");
    assert_eq!(v["status"], "Expired");
}

#[tokio::test]
async fn session_exchange_requires_valid_shared_secret() {
    init_test_config();
    let state = make_state_with_storage().await;
    let err = post_session_exchange(
        State(state),
        make_exchange_headers("bad-secret"),
        Json(make_exchange_request()),
    )
    .await
    .expect_err("should reject invalid shared secret");
    let resp = err.into_response();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn session_exchange_returns_bearer_token() {
    init_test_config();
    let state = make_state_with_storage().await;
    let (status, Json(resp)) = post_session_exchange(
        State(state),
        make_exchange_headers("test-exchange-secret"),
        Json(make_exchange_request()),
    )
    .await
    .expect("exchange success");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(resp.token_type, "Bearer");
    assert!(resp.expires_in > 0);
    let claims = decode_claims(&resp.access_token);
    assert!(claims.get("sub").and_then(|v| v.as_str()).is_some());
    assert!(claims.get("jti").and_then(|v| v.as_str()).is_some());
}

#[tokio::test]
async fn session_logout_current_blacklists_token() {
    init_test_config();
    let state = make_state_with_storage().await;
    let state_for_check = state.clone();
    let token = exchange_token("test-exchange-secret").await;
    let claims = decode_claims(&token);
    let jti = claims
        .get("jti")
        .and_then(|v| v.as_str())
        .expect("jti claim")
        .to_string();

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        axum::http::HeaderValue::from_str(&format!("Bearer {token}")).expect("auth header"),
    );

    let (status, Json(resp)) = post_session_logout(
        State(state),
        headers,
        Json(SessionLogoutRequest {
            scope: SessionLogoutScope::Current,
        }),
    )
    .await
    .expect("logout current success");

    assert_eq!(status, StatusCode::OK);
    assert_eq!(resp.scope, SessionLogoutScope::Current);
    assert_eq!(resp.revoked_jti, jti);
    assert!(resp.logout_before.is_none());

    let storage = state_for_check
        .stats_storage
        .expect("stats storage missing");
    let now = chrono::Utc::now().to_rfc3339();
    let blacklisted = storage
        .is_token_blacklisted(&resp.revoked_jti, &now)
        .await
        .expect("query blacklist");
    assert!(blacklisted);
}

#[tokio::test]
async fn session_logout_all_writes_logout_gate() {
    init_test_config();
    let state = make_state_with_storage().await;
    let state_for_check = state.clone();
    let token = exchange_token("test-exchange-secret").await;
    let claims = decode_claims(&token);
    let user_hash = claims
        .get("sub")
        .and_then(|v| v.as_str())
        .expect("sub claim")
        .to_string();

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        axum::http::HeaderValue::from_str(&format!("Bearer {token}")).expect("auth header"),
    );

    let (status, Json(resp)) = post_session_logout(
        State(state),
        headers,
        Json(SessionLogoutRequest {
            scope: SessionLogoutScope::All,
        }),
    )
    .await
    .expect("logout all success");

    assert_eq!(status, StatusCode::OK);
    assert_eq!(resp.scope, SessionLogoutScope::All);
    assert!(resp.logout_before.is_some());

    let storage = state_for_check
        .stats_storage
        .expect("stats storage missing");
    let now = chrono::Utc::now().to_rfc3339();
    let gate = storage
        .get_logout_gate(&user_hash, &now)
        .await
        .expect("query logout gate");
    assert!(gate.is_some());
}

#[tokio::test]
async fn session_cleanup_removes_expired_records() {
    init_test_config();
    let state = make_state_with_storage().await;
    let storage = state.stats_storage.expect("stats storage missing");
    let now = chrono::Utc::now();

    storage
        .add_token_blacklist(
            "expired-jti",
            &(now - chrono::Duration::seconds(1)).to_rfc3339(),
            &now.to_rfc3339(),
        )
        .await
        .expect("insert expired blacklist");
    storage
        .add_token_blacklist(
            "live-jti",
            &(now + chrono::Duration::hours(1)).to_rfc3339(),
            &now.to_rfc3339(),
        )
        .await
        .expect("insert live blacklist");

    storage
        .upsert_logout_gate(
            "expired-user",
            &(now - chrono::Duration::seconds(10)).to_rfc3339(),
            &(now - chrono::Duration::seconds(1)).to_rfc3339(),
            &now.to_rfc3339(),
        )
        .await
        .expect("insert expired gate");
    storage
        .upsert_logout_gate(
            "live-user",
            &now.to_rfc3339(),
            &(now + chrono::Duration::hours(1)).to_rfc3339(),
            &now.to_rfc3339(),
        )
        .await
        .expect("insert live gate");

    let (blacklist_deleted, gate_deleted) = storage
        .cleanup_expired_session_records(&now.to_rfc3339())
        .await
        .expect("cleanup expired records");

    assert!(blacklist_deleted >= 1);
    assert!(gate_deleted >= 1);

    let expired_exists = storage
        .is_token_blacklisted("expired-jti", &now.to_rfc3339())
        .await
        .expect("query expired blacklist");
    assert!(!expired_exists);

    let live_exists = storage
        .is_token_blacklisted("live-jti", &now.to_rfc3339())
        .await
        .expect("query live blacklist");
    assert!(live_exists);

    let expired_gate = storage
        .get_logout_gate("expired-user", &now.to_rfc3339())
        .await
        .expect("query expired gate");
    assert!(expired_gate.is_none());

    let live_gate = storage
        .get_logout_gate("live-user", &now.to_rfc3339())
        .await
        .expect("query live gate");
    assert!(live_gate.is_some());
}
