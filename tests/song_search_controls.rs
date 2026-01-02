use std::sync::Arc;

use axum::{
    Router,
    body::{Body, Bytes},
    http::{Request, StatusCode},
};
use moka::future::Cache;
use tokio::sync::Semaphore;
use tower::ServiceExt;

use phi_backend::{
    config::{TapTapConfig, TapTapMultiConfig, TapTapVersion},
    features::{
        auth::client::TapTapClient,
        song::models::{SongCatalog, SongInfo},
    },
    startup::chart_loader::ChartConstants,
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

fn new_test_state(song_catalog: SongCatalog) -> AppState {
    let taptap_cfg = dummy_taptap_cfg(TapTapVersion::CN);
    let taptap_client = TapTapClient::new(&taptap_cfg).expect("TapTapClient::new");

    let bn_image_cache: Cache<String, Bytes> = Cache::builder().max_capacity(1024).build();
    let song_image_cache: Cache<String, Bytes> = Cache::builder().max_capacity(1024).build();

    AppState {
        chart_constants: Arc::new(Default::default()),
        song_catalog: Arc::new(song_catalog),
        taptap_client: Arc::new(taptap_client),
        qrcode_service: Arc::new(Default::default()),
        stats: None,
        stats_storage: None,
        render_semaphore: Arc::new(Semaphore::new(1)),
        bn_image_cache,
        song_image_cache,
    }
}

fn build_app(state: AppState) -> Router {
    // 贴近生产部署：songs/* 实际挂在 /api/v2 下
    Router::<AppState>::new()
        .nest("/api/v2", phi_backend::features::song::create_song_router())
        .with_state(state)
}

fn chart_constants_none() -> ChartConstants {
    ChartConstants {
        ez: None,
        hd: None,
        in_level: None,
        at: None,
    }
}

#[test]
fn song_catalog_search_is_stably_ordered_for_same_name() {
    let mut catalog = SongCatalog::default();

    let a = Arc::new(SongInfo {
        id: "a".to_string(),
        name: "Alpha".to_string(),
        composer: "c".to_string(),
        illustrator: "i".to_string(),
        chart_constants: chart_constants_none(),
    });
    let b = Arc::new(SongInfo {
        id: "b".to_string(),
        name: "Alpha".to_string(),
        composer: "c".to_string(),
        illustrator: "i".to_string(),
        chart_constants: chart_constants_none(),
    });

    // 故意用“看起来会打乱”的顺序插入
    catalog.by_id.insert(b.id.clone(), Arc::clone(&b));
    catalog.by_id.insert(a.id.clone(), Arc::clone(&a));

    catalog
        .by_name
        .entry("Alpha".to_string())
        .or_insert_with(Vec::new)
        .extend([Arc::clone(&b), Arc::clone(&a)]);

    catalog.rebuild_search_cache();

    let items = catalog.search("Alpha");
    let ids: Vec<&str> = items.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec!["a", "b"]);
}

#[tokio::test]
async fn songs_search_default_limit_is_applied() {
    let mut catalog = SongCatalog::default();

    for i in 0..50 {
        let id = format!("id-{i:03}");
        let name = format!("song-{i:03}");
        let info = Arc::new(SongInfo {
            id: id.clone(),
            name,
            composer: "c".to_string(),
            illustrator: "i".to_string(),
            chart_constants: chart_constants_none(),
        });
        catalog.by_id.insert(id, Arc::clone(&info));
    }
    catalog.rebuild_search_cache();

    let app = build_app(new_test_state(catalog));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v2/songs/search?q=song")
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
    assert_eq!(v["items"].as_array().expect("items array").len(), 20);
    assert_eq!(v["total"].as_u64().expect("total"), 50);
    assert_eq!(v["limit"].as_u64().expect("limit"), 20);
    assert_eq!(v["offset"].as_u64().expect("offset"), 0);
    assert_eq!(v["hasMore"].as_bool().expect("hasMore"), true);
    assert_eq!(v["nextOffset"].as_u64().expect("nextOffset"), 20);
}

#[tokio::test]
async fn songs_search_limit_is_clamped_to_max() {
    let mut catalog = SongCatalog::default();

    for i in 0..150 {
        let id = format!("id-{i:03}");
        let name = format!("song-{i:03}");
        let info = Arc::new(SongInfo {
            id: id.clone(),
            name,
            composer: "c".to_string(),
            illustrator: "i".to_string(),
            chart_constants: chart_constants_none(),
        });
        catalog.by_id.insert(id, Arc::clone(&info));
    }
    catalog.rebuild_search_cache();

    let app = build_app(new_test_state(catalog));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v2/songs/search?q=song&limit=1000")
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
    assert_eq!(v["items"].as_array().expect("items array").len(), 100);
    assert_eq!(v["total"].as_u64().expect("total"), 150);
    assert_eq!(v["limit"].as_u64().expect("limit"), 100);
    assert_eq!(v["offset"].as_u64().expect("offset"), 0);
    assert_eq!(v["hasMore"].as_bool().expect("hasMore"), true);
    assert_eq!(v["nextOffset"].as_u64().expect("nextOffset"), 100);
}

#[tokio::test]
async fn songs_search_offset_paginates_and_reports_next_offset() {
    let mut catalog = SongCatalog::default();

    for i in 0..50 {
        let id = format!("id-{i:03}");
        let name = format!("song-{i:03}");
        let info = Arc::new(SongInfo {
            id: id.clone(),
            name,
            composer: "c".to_string(),
            illustrator: "i".to_string(),
            chart_constants: chart_constants_none(),
        });
        catalog.by_id.insert(id, Arc::clone(&info));
    }
    catalog.rebuild_search_cache();

    let app = build_app(new_test_state(catalog));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v2/songs/search?q=song&limit=20&offset=20")
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

    assert_eq!(v["items"].as_array().expect("items array").len(), 20);
    assert_eq!(v["total"].as_u64().expect("total"), 50);
    assert_eq!(v["limit"].as_u64().expect("limit"), 20);
    assert_eq!(v["offset"].as_u64().expect("offset"), 20);
    assert_eq!(v["hasMore"].as_bool().expect("hasMore"), true);
    assert_eq!(v["nextOffset"].as_u64().expect("nextOffset"), 40);

    // 由于缓存已稳定排序，第二页第一条应为 song-020
    assert_eq!(v["items"][0]["id"].as_str().expect("id"), "id-020");
}

#[tokio::test]
async fn songs_search_last_page_has_no_next_offset() {
    let mut catalog = SongCatalog::default();

    for i in 0..50 {
        let id = format!("id-{i:03}");
        let name = format!("song-{i:03}");
        let info = Arc::new(SongInfo {
            id: id.clone(),
            name,
            composer: "c".to_string(),
            illustrator: "i".to_string(),
            chart_constants: chart_constants_none(),
        });
        catalog.by_id.insert(id, Arc::clone(&info));
    }
    catalog.rebuild_search_cache();

    let app = build_app(new_test_state(catalog));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v2/songs/search?q=song&limit=20&offset=40")
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

    assert_eq!(v["items"].as_array().expect("items array").len(), 10);
    assert_eq!(v["total"].as_u64().expect("total"), 50);
    assert_eq!(v["limit"].as_u64().expect("limit"), 20);
    assert_eq!(v["offset"].as_u64().expect("offset"), 40);
    assert_eq!(v["hasMore"].as_bool().expect("hasMore"), false);
    assert!(v.get("nextOffset").is_none() || v["nextOffset"].is_null());
}

#[tokio::test]
async fn songs_search_limit_zero_is_validation_error() {
    let mut catalog = SongCatalog::default();
    catalog.rebuild_search_cache();

    let app = build_app(new_test_state(catalog));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v2/songs/search?q=song&limit=0")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request");

    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn songs_search_query_too_long_is_validation_error() {
    let mut catalog = SongCatalog::default();
    catalog.rebuild_search_cache();

    let app = build_app(new_test_state(catalog));

    let too_long_q = "a".repeat(129);
    let uri = format!("/api/v2/songs/search?q={too_long_q}");
    let resp = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .expect("request");

    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}
