use std::sync::Arc;

use axum::{
    Router,
    body::{Body, Bytes},
    http::{Request, StatusCode},
};
use moka::future::Cache;
use tokio::sync::Semaphore;
use tower::ServiceExt;
use uuid::Uuid;

use phi_backend::{
    config::{TapTapConfig, TapTapMultiConfig, TapTapVersion},
    features::{
        auth::client::TapTapClient, leaderboard::handler::create_leaderboard_router,
        song::models::SongCatalog, stats::storage::StatsStorage,
    },
    state::AppState,
};

fn dummy_taptap_cfg(default_version: TapTapVersion) -> TapTapMultiConfig {
    TapTapMultiConfig {
        cn: TapTapConfig {
            device_code_endpoint: "http://example.invalid/device/code".to_string(),
            token_endpoint: "http://example.invalid/token".to_string(),
            user_info_endpoint: "http://example.invalid/userinfo".to_string(),
            leancloud_base_url: "http://example.invalid/leancloud".to_string(),
            leancloud_app_id: "cn-app-id".to_string(),
            leancloud_app_key: "cn-app-key".to_string(),
        },
        global: TapTapConfig {
            device_code_endpoint: "http://example.invalid/device/code".to_string(),
            token_endpoint: "http://example.invalid/token".to_string(),
            user_info_endpoint: "http://example.invalid/userinfo".to_string(),
            leancloud_base_url: "http://example.invalid/leancloud".to_string(),
            leancloud_app_id: "global-app-id".to_string(),
            leancloud_app_key: "global-app-key".to_string(),
        },
        default_version,
    }
}

fn new_test_state(storage: Arc<StatsStorage>) -> AppState {
    let taptap_cfg = dummy_taptap_cfg(TapTapVersion::CN);
    let taptap_client = TapTapClient::new(&taptap_cfg).expect("TapTapClient::new");

    let bn_image_cache: Cache<String, Bytes> = Cache::builder().max_capacity(1024).build();
    let song_image_cache: Cache<String, Bytes> = Cache::builder().max_capacity(1024).build();

    AppState {
        chart_constants: Arc::new(Default::default()),
        song_catalog: Arc::new(SongCatalog::default()),
        taptap_client: Arc::new(taptap_client),
        qrcode_service: Arc::new(Default::default()),
        stats: None,
        stats_storage: Some(storage),
        render_semaphore: Arc::new(Semaphore::new(1)),
        bn_image_cache,
        song_image_cache,
    }
}

fn build_app(state: AppState) -> Router {
    // 贴近生产部署：leaderboard/* 实际挂在 /api/v2 下
    Router::<AppState>::new()
        .nest("/api/v2", create_leaderboard_router())
        .with_state(state)
}

async fn seed_leaderboard_db(storage: &StatsStorage) {
    storage.init_schema().await.expect("init_schema");

    let now = chrono::Utc::now().to_rfc3339();

    // 3 个公开用户，保证 limit=2 时存在 nextAfter*
    let users = [
        ("u1", "Alice", 15.0),
        ("u2", "Bob", 14.0),
        ("u3", "Carol", 13.0),
    ];

    for (user_hash, alias, score) in users {
        sqlx::query(
            "INSERT INTO user_profile(user_hash, alias, is_public, show_rks_composition, show_best_top3, show_ap_top3, user_kind, created_at, updated_at)
             VALUES(?,?,?,?,?,?,?,?,?)",
        )
        .bind(user_hash)
        .bind(alias)
        .bind(1_i64)
        .bind(1_i64)
        .bind(1_i64)
        .bind(1_i64)
        .bind(Option::<String>::None)
        .bind(&now)
        .bind(&now)
        .execute(&storage.pool)
        .await
        .expect("insert user_profile");

        storage
            .upsert_leaderboard_rks(user_hash, score, Some("k"), 0.0, false, &now)
            .await
            .expect("upsert leaderboard_rks");

        let best3 = r#"[{"song":"Best Song","difficulty":"AT","acc":99.43,"rks":15.12}]"#;
        let ap3 = r#"[{"song":"AP Song","difficulty":"IN","acc":100.0,"rks":13.45}]"#;
        storage
            .upsert_details(user_hash, None, Some(best3), Some(ap3), &now)
            .await
            .expect("upsert leaderboard_details");
    }
}

async fn seed_many_public_users(storage: &StatsStorage, n: usize) {
    storage.init_schema().await.expect("init_schema");

    let now = chrono::Utc::now().to_rfc3339();
    for i in 0..n {
        let user_hash = format!("u{i:04}");
        let alias = format!("User{i:04}");
        let score = 1000.0 - i as f64;

        sqlx::query(
            "INSERT INTO user_profile(user_hash, alias, is_public, show_rks_composition, show_best_top3, show_ap_top3, user_kind, created_at, updated_at)
             VALUES(?,?,?,?,?,?,?,?,?)",
        )
        .bind(&user_hash)
        .bind(&alias)
        .bind(1_i64)
        .bind(1_i64)
        .bind(0_i64)
        .bind(0_i64)
        .bind(Option::<String>::None)
        .bind(&now)
        .bind(&now)
        .execute(&storage.pool)
        .await
        .expect("insert user_profile");

        storage
            .upsert_leaderboard_rks(&user_hash, score, Some("k"), 0.0, false, &now)
            .await
            .expect("upsert leaderboard_rks");
    }
}

#[tokio::test]
async fn leaderboard_top_masks_next_after_user_and_supports_lite() {
    let path = format!("./resources/test_lb_endpoint_{}.db", Uuid::new_v4());
    if std::fs::metadata(&path).is_ok() {
        let _ = std::fs::remove_file(&path);
    }

    let storage = StatsStorage::connect_sqlite(&path, false)
        .await
        .expect("connect_sqlite");
    seed_leaderboard_db(&storage).await;

    let app = build_app(new_test_state(Arc::new(storage)));

    // 默认：返回 BestTop3/APTop3，并且 nextAfterUser 不泄露原始 user_hash
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v2/leaderboard/rks/top?limit=2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request");
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("read body");
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");

    let items = v["items"].as_array().expect("items array");
    assert_eq!(items.len(), 2);
    assert!(items[0].get("bestTop3").is_some());
    assert!(items[0].get("apTop3").is_some());

    // nextAfterUser 应与最后一条 items.user 一致（均为去敏化前缀）
    assert_eq!(v["nextAfterUser"], items[1]["user"]);
    assert!(
        v["nextAfterUser"]
            .as_str()
            .unwrap_or_default()
            .ends_with("****")
    );

    // lite=true：不返回 BestTop3/APTop3
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v2/leaderboard/rks/top?limit=2&lite=true")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request");
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("read body");
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");
    let items = v["items"].as_array().expect("items array");
    assert_eq!(items.len(), 2);

    for it in items {
        assert!(it.get("bestTop3").is_none());
        assert!(it.get("apTop3").is_none());
    }
    assert_eq!(v["nextAfterUser"], items[1]["user"]);
    assert!(
        v["nextAfterUser"]
            .as_str()
            .unwrap_or_default()
            .ends_with("****")
    );
}

#[tokio::test]
async fn leaderboard_top_limit_cap_is_relaxed_in_lite_mode() {
    let path = format!("./resources/test_lb_top_limit_{}.db", Uuid::new_v4());
    if std::fs::metadata(&path).is_ok() {
        let _ = std::fs::remove_file(&path);
    }

    let storage = StatsStorage::connect_sqlite(&path, false)
        .await
        .expect("connect_sqlite");
    seed_many_public_users(&storage, 300).await;

    let app = build_app(new_test_state(Arc::new(storage)));

    // 非 lite：limit 仍然 clamp 到 200，避免放大 BestTop3/APTop3 查询与 JSON 反序列化成本
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v2/leaderboard/rks/top?limit=999")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request");
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("read body");
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");
    let items = v["items"].as_array().expect("items array");
    assert_eq!(items.len(), 200);
    assert_eq!(v["total"].as_i64().unwrap_or_default(), 300);

    // lite=true：允许返回 >200 条（本用例数据为 300）
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v2/leaderboard/rks/top?limit=999&lite=true")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request");
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("read body");
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");
    let items = v["items"].as_array().expect("items array");
    assert_eq!(items.len(), 300);

    for it in items {
        assert!(it.get("bestTop3").is_none());
        assert!(it.get("apTop3").is_none());
    }
}

#[tokio::test]
async fn leaderboard_by_rank_masks_next_after_user_and_supports_lite() {
    let path = format!("./resources/test_lb_by_rank_endpoint_{}.db", Uuid::new_v4());
    if std::fs::metadata(&path).is_ok() {
        let _ = std::fs::remove_file(&path);
    }

    let storage = StatsStorage::connect_sqlite(&path, false)
        .await
        .expect("connect_sqlite");
    seed_leaderboard_db(&storage).await;

    let app = build_app(new_test_state(Arc::new(storage)));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v2/leaderboard/rks/by-rank?start=1&count=2&lite=true")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request");
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("read body");
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");
    let items = v["items"].as_array().expect("items array");
    assert_eq!(items.len(), 2);
    for it in items {
        assert!(it.get("bestTop3").is_none());
        assert!(it.get("apTop3").is_none());
    }
    assert_eq!(v["nextAfterUser"], items[1]["user"]);
    assert!(
        v["nextAfterUser"]
            .as_str()
            .unwrap_or_default()
            .ends_with("****")
    );
}
