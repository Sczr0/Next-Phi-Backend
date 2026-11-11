#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::similar_names,
    clippy::default_trait_access,
    clippy::too_many_lines
)]

use std::sync::{Arc, Once};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine;
use moka::future::Cache;
use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;
use tower::ServiceExt;
use uuid::Uuid;

use phi_backend::{
    config::{AppConfig, DelegationConfig, TapTapConfig, TapTapMultiConfig, TapTapVersion},
    features::{
        auth::client::TapTapClient, song::models::SongCatalog, stats::storage::StatsStorage,
    },
    state::AppState,
};

const REDIRECT_URI: &str = "http://client.test/callback";
const CLIENT_SECRET: &str = "delegation-test-secret";
const VERIFIER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn init_config() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        unsafe {
            std::env::set_var("APP_STATS_USER_HASH_SALT", "test-salt");
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

fn dummy_taptap(app: &str, key: &str) -> TapTapConfig {
    TapTapConfig {
        device_code_endpoint: "http://example.invalid/device/code".into(),
        token_endpoint: "http://example.invalid/token".into(),
        user_info_endpoint: "http://example.invalid/userinfo".into(),
        leancloud_base_url: "http://example.invalid/leancloud".into(),
        leancloud_app_id: app.into(),
        leancloud_app_key: key.into(),
    }
}

fn make_state(storage: Arc<StatsStorage>) -> AppState {
    let taptap = TapTapMultiConfig {
        cn: dummy_taptap("cn-app", "cn-key"),
        global: dummy_taptap("global-app", "global-key"),
        default_version: TapTapVersion::CN,
    };
    AppState {
        chart_constants: Arc::new(Default::default()),
        song_catalog: Arc::new(SongCatalog::default()),
        taptap_client: Arc::new(TapTapClient::new(&taptap).expect("taptap client")),
        qrcode_service: Arc::new(
            phi_backend::features::auth::qrcode_service::QrCodeService::default(),
        ),
        stats: None,
        stats_storage: Some(storage),
        render_semaphore: Arc::new(Semaphore::new(1)),
        bn_image_cache: Cache::builder().max_capacity(8).build(),
        song_image_cache: Cache::builder().max_capacity(8).build(),
    }
}

async fn body_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json")
}

fn pkce_challenge() -> String {
    let digest = Sha256::digest(VERIFIER.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

#[tokio::test]
async fn delegation_flow_issues_refreshes_and_revokes() {
    init_config();
    let db_path = format!("./resources/test_stats_delegation_{}.db", Uuid::new_v4());
    let stats = StatsStorage::connect_sqlite(&db_path, true)
        .await
        .expect("stats db");
    stats.init_schema().await.expect("stats schema");
    let state = make_state(Arc::new(stats));
    let delegation = DelegationConfig {
        client_secret: CLIENT_SECRET.to_string(),
        redirect_uri: REDIRECT_URI.to_string(),
        consent_url: "http://lilith.test/consent".to_string(),
        credential_key: "1111111111111111111111111111111111111111111111111111111111111111"
            .to_string(),
        sqlite_path: "./resources/test_delegation.db".to_string(),
        ..Default::default()
    };
    phi_backend::features::delegation::init(&delegation)
        .await
        .expect("delegation init");

    let api_router = axum::Router::<AppState>::new()
        .nest(
            "/auth",
            phi_backend::features::auth::handler::create_auth_router(),
        )
        .merge(phi_backend::features::delegation::router())
        .merge(phi_backend::features::save::create_save_router())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            phi_backend::features::auth::bearer::bearer_auth_middleware,
        ));
    let app = axum::Router::new()
        .nest("/api/v2", api_router)
        .with_state(state);

    let exchange = Request::builder()
        .method("POST")
        .uri("/api/v2/auth/session/exchange")
        .header("content-type", "application/json")
        .header("x-exchange-secret", "test-exchange-secret")
        .body(Body::from(r#"{"sessionToken":"r:test-session"}"#))
        .expect("exchange request");
    let exchange = app.clone().oneshot(exchange).await.expect("exchange");
    assert_eq!(exchange.status(), StatusCode::OK);
    let exchange_json = body_json(exchange).await;
    let session_token = exchange_json["accessToken"]
        .as_str()
        .expect("session token");

    let challenge = pkce_challenge();
    let authorize_uri = format!(
        "/api/v2/delegations/authorize?response_type=code&client_id=re0auth&redirect_uri={}&state=abc&code_challenge={}&code_challenge_method=S256&scope=phigros.query.save.read",
        urlencoding(REDIRECT_URI),
        challenge
    );
    let authorize = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(authorize_uri)
                .body(Body::empty())
                .expect("authorize request"),
        )
        .await
        .expect("authorize");
    assert_eq!(authorize.status(), StatusCode::TEMPORARY_REDIRECT);
    let consent = authorize
        .headers()
        .get("location")
        .and_then(|value| value.to_str().ok())
        .expect("consent location");
    let request_id = consent
        .split("request_id=")
        .nth(1)
        .expect("request id")
        .split('&')
        .next()
        .expect("request id value");

    let approve = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v2/delegations/requests/{request_id}/approve"))
                .header("authorization", format!("Bearer {session_token}"))
                .body(Body::empty())
                .expect("approve request"),
        )
        .await
        .expect("approve");
    assert_eq!(approve.status(), StatusCode::OK);
    let approve_json = body_json(approve).await;
    let code = approve_json["redirect_url"]
        .as_str()
        .expect("redirect")
        .split("code=")
        .nth(1)
        .expect("code")
        .split('&')
        .next()
        .expect("code value");

    let form = format!(
        "grant_type=authorization_code&code={code}&redirect_uri={}&client_id=re0auth&client_secret={CLIENT_SECRET}&code_verifier={VERIFIER}",
        urlencoding(REDIRECT_URI)
    );
    let token = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v2/delegations/token")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(form))
                .expect("token request"),
        )
        .await
        .expect("token");
    assert_eq!(token.status(), StatusCode::OK);
    let token_json = body_json(token).await;
    let access = token_json["access_token"]
        .as_str()
        .expect("access")
        .to_string();
    let refresh = token_json["refresh_token"]
        .as_str()
        .expect("refresh")
        .to_string();

    let me = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v2/delegations/me")
                .header("authorization", format!("Bearer {access}"))
                .body(Body::empty())
                .expect("me request"),
        )
        .await
        .expect("me");
    assert_eq!(me.status(), StatusCode::OK);

    let refresh_form = format!(
        "grant_type=refresh_token&refresh_token={refresh}&client_id=re0auth&client_secret={CLIENT_SECRET}"
    );
    let refreshed = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v2/delegations/token")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(refresh_form))
                .expect("refresh request"),
        )
        .await
        .expect("refresh");
    assert_eq!(refreshed.status(), StatusCode::OK);

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::AUTHORIZATION,
        format!("Bearer {access}").parse().expect("auth header"),
    );
    assert_eq!(
        phi_backend::features::delegation::handlers::authenticate_bearer(
            &headers,
            phi_backend::features::delegation::handlers::SAVE_SCOPE
        )
        .await
        .expect("delegation auth")
        .as_deref(),
        Some("r:test-session")
    );

    let revoke_form = format!("token={access}&client_id=re0auth&client_secret={CLIENT_SECRET}");
    let revoked = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v2/delegations/revoke")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(revoke_form))
                .expect("revoke request"),
        )
        .await
        .expect("revoke");
    assert_eq!(revoked.status(), StatusCode::OK);

    let after = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v2/delegations/me")
                .header("authorization", format!("Bearer {access}"))
                .body(Body::empty())
                .expect("me after revoke"),
        )
        .await
        .expect("me after revoke");
    assert_eq!(after.status(), StatusCode::UNAUTHORIZED);

    let save = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v2/save")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {access}"))
                .body(Body::from("{}"))
                .expect("save after revoke"),
        )
        .await
        .expect("save after revoke");
    assert_eq!(save.status(), StatusCode::UNAUTHORIZED);
}

fn urlencoding(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}
