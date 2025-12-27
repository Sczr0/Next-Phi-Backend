use std::sync::Arc;

use axum::body::{Bytes, to_bytes};
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use moka::future::Cache;
use tokio::sync::Semaphore;

use phi_backend::config::{TapTapConfig, TapTapMultiConfig, TapTapVersion};
use phi_backend::features::auth::client::TapTapClient;
use phi_backend::features::auth::qrcode_service::QrCodeService;
use phi_backend::features::song::models::SongCatalog;
use phi_backend::state::AppState;

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
