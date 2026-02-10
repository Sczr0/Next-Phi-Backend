use std::sync::{Arc, Once};

use axum::{
    Router,
    body::Bytes,
    http::{Request, StatusCode},
};
use moka::future::Cache;
use tokio::sync::Semaphore;
use tower::ServiceExt;
use uuid::Uuid;

use phi_backend::{
    config::{AppConfig, TapTapConfig, TapTapMultiConfig, TapTapVersion},
    features::{
        auth::{client::TapTapClient, handler::create_auth_router},
        rks::handler::create_rks_router,
        save::create_save_router,
        song::models::SongCatalog,
        stats::storage::StatsStorage,
    },
    state::AppState,
};

fn init_test_config() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        unsafe {
            std::env::set_var("APP_STATS_USER_HASH_SALT", "test-user-hash-salt");
            std::env::set_var("APP_SESSION_JWT_SECRET", "test-jwt-secret");
            std::env::set_var("APP_SESSION_EXCHANGE_SHARED_SECRET", "test-exchange-secret");
            std::env::set_var(
                "APP_SESSION_AUTH_EMBED_SECRET",
                "test-embed-secret-1234567890",
            );
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

fn make_state(storage: Arc<StatsStorage>) -> AppState {
    let taptap_cfg = TapTapMultiConfig {
        cn: dummy_taptap_config("cn-app-id", "cn-app-key"),
        global: dummy_taptap_config("global-app-id", "global-app-key"),
        default_version: TapTapVersion::CN,
    };
    let taptap_client = Arc::new(TapTapClient::new(&taptap_cfg).expect("TapTapClient::new"));

    let bn_image_cache: Cache<String, Bytes> = Cache::builder().max_capacity(16).build();
    let song_image_cache: Cache<String, Bytes> = Cache::builder().max_capacity(16).build();

    AppState {
        chart_constants: Arc::new(Default::default()),
        song_catalog: Arc::new(SongCatalog::default()),
        taptap_client,
        qrcode_service: Arc::new(Default::default()),
        stats: None,
        stats_storage: Some(storage),
        render_semaphore: Arc::new(Semaphore::new(1)),
        bn_image_cache,
        song_image_cache,
    }
}

fn make_app(state: AppState) -> Router {
    let api_router = Router::<AppState>::new()
        .nest("/auth", create_auth_router())
        .merge(create_save_router())
        .merge(create_rks_router());

    let api_router = api_router.layer(axum::middleware::from_fn_with_state(
        state.clone(),
        phi_backend::features::auth::bearer::bearer_auth_middleware,
    ));

    Router::<AppState>::new()
        .nest("/api/v2", api_router)
        .with_state(state)
}

#[tokio::test]
async fn bearer_can_call_rks_history_without_body_auth() {
    init_test_config();
    let db_path = format!("./resources/test_bearer_auth_{}.db", Uuid::new_v4());
    let storage = StatsStorage::connect_sqlite(&db_path, true)
        .await
        .expect("connect sqlite");
    storage.init_schema().await.expect("init schema");

    let app = make_app(make_state(Arc::new(storage)));

    let exchange_body = serde_json::json!({
        "sessionToken": "r:test-session-token"
    });
    let exchange_req = Request::builder()
        .method("POST")
        .uri("/api/v2/auth/session/exchange")
        .header("content-type", "application/json")
        .header("x-exchange-secret", "test-exchange-secret")
        .body(axum::body::Body::from(exchange_body.to_string()))
        .expect("build exchange request");
    let exchange_resp = app
        .clone()
        .oneshot(exchange_req)
        .await
        .expect("exchange response");
    assert_eq!(exchange_resp.status(), StatusCode::OK);
    let exchange_bytes = axum::body::to_bytes(exchange_resp.into_body(), usize::MAX)
        .await
        .expect("read exchange body");
    let exchange_json: serde_json::Value =
        serde_json::from_slice(&exchange_bytes).expect("parse exchange body");
    let token = exchange_json
        .get("accessToken")
        .and_then(|v| v.as_str())
        .expect("access token");
    let embedded_auth = phi_backend::features::auth::bearer::decode_embedded_auth(token)
        .expect("decode embedded auth from token");
    assert!(embedded_auth.session_token.is_some());
    let cfg = &AppConfig::global().session;
    let claims = phi_backend::features::auth::bearer::decode_access_token(token, cfg, true)
        .expect("decode bearer claims");
    assert!(!claims.sub.is_empty());

    let rks_body = serde_json::json!({
        "auth": {},
        "limit": 10,
        "offset": 0
    });
    let rks_req = Request::builder()
        .method("POST")
        .uri("/api/v2/rks/history")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(axum::body::Body::from(rks_body.to_string()))
        .expect("build rks request");
    let rks_resp = app.clone().oneshot(rks_req).await.expect("rks response");

    let rks_status = rks_resp.status();
    let rks_bytes = axum::body::to_bytes(rks_resp.into_body(), usize::MAX)
        .await
        .expect("read rks body");
    let rks_text = String::from_utf8_lossy(&rks_bytes);

    assert!(
        rks_text.contains("\"items\"") && rks_text.contains("\"total\""),
        "unexpected rks response body: {rks_text}"
    );
    assert_eq!(rks_status, StatusCode::OK);
}
