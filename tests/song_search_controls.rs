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
        chart_constants: Arc::new(std::collections::HashMap::default()),
        song_catalog: Arc::new(song_catalog),
        taptap_client: Arc::new(taptap_client),
        qrcode_service: Arc::new(
            phi_backend::features::auth::qrcode_service::QrCodeService::default(),
        ),
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
        .nest(
            "/api/v2",
            phi_backend::features::song::create_song_router().layer(
                axum::middleware::from_fn_with_state(
                    state.clone(),
                    phi_backend::features::auth::bearer::bearer_auth_middleware,
                ),
            ),
        )
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
        .or_default()
        .extend([Arc::clone(&b), Arc::clone(&a)]);

    catalog.rebuild_search_cache();

    let items = catalog.search("Alpha");
    let ids: Vec<&str> = items.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec!["a", "b"]);
}

#[test]
fn song_catalog_search_page_matches_id_case_insensitively() {
    let mut catalog = SongCatalog::default();

    let info = Arc::new(SongInfo {
        id: "Stasis.Maozon".to_string(),
        name: "Stasis".to_string(),
        composer: "Maozon".to_string(),
        illustrator: "i".to_string(),
        chart_constants: chart_constants_none(),
    });

    catalog.by_id.insert(info.id.clone(), Arc::clone(&info));
    catalog.rebuild_search_cache();

    let (items, total) = catalog.search_page("stasis.maozon", 0, 20);
    assert_eq!(total, 1);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "Stasis.Maozon");

    let unique = catalog
        .search_unique("stasis.maozon")
        .expect("case-insensitive id should match");
    assert_eq!(unique.id, "Stasis.Maozon");
}

#[test]
fn song_catalog_search_matches_normalized_name_without_spaces_and_symbols() {
    let mut catalog = SongCatalog::default();

    let info = Arc::new(SongInfo {
        id: "quo".to_string(),
        name: "君往何处 (Quo Vadis)".to_string(),
        composer: "M2U".to_string(),
        illustrator: "i".to_string(),
        chart_constants: chart_constants_none(),
    });

    catalog.by_id.insert(info.id.clone(), Arc::clone(&info));
    catalog.rebuild_search_cache();

    let items = catalog.search("QuoVadis");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "quo");
}

#[test]
fn song_catalog_search_fuzzy_matches_japanese_name_when_normalized_match_fails() {
    let mut catalog = SongCatalog::default();

    let info = Arc::new(SongInfo {
        id: "amb".to_string(),
        name: "アンビバレンス".to_string(),
        composer: "c".to_string(),
        illustrator: "i".to_string(),
        chart_constants: chart_constants_none(),
    });

    catalog.by_id.insert(info.id.clone(), Arc::clone(&info));
    catalog.rebuild_search_cache();

    let items = catalog.search("アンバレンス");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "amb");
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
    assert!(v["hasMore"].as_bool().expect("hasMore"));
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
    assert!(v["hasMore"].as_bool().expect("hasMore"));
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
    assert!(v["hasMore"].as_bool().expect("hasMore"));
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
    assert!(!v["hasMore"].as_bool().expect("hasMore"));
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

#[tokio::test]
async fn songs_search_mode_and_intersects_multiple_terms() {
    let mut catalog = SongCatalog::default();

    let target = Arc::new(SongInfo {
        id: "target".to_string(),
        name: "Burning Start".to_string(),
        composer: "Noah".to_string(),
        illustrator: "i".to_string(),
        chart_constants: chart_constants_none(),
    });
    let miss_name = Arc::new(SongInfo {
        id: "miss-name".to_string(),
        name: "Burning".to_string(),
        composer: "Other".to_string(),
        illustrator: "i".to_string(),
        chart_constants: chart_constants_none(),
    });
    let miss_composer = Arc::new(SongInfo {
        id: "miss-composer".to_string(),
        name: "Start".to_string(),
        composer: "Noah".to_string(),
        illustrator: "i".to_string(),
        chart_constants: chart_constants_none(),
    });

    for song in [&target, &miss_name, &miss_composer] {
        catalog.by_id.insert(song.id.clone(), Arc::clone(song));
    }
    catalog.rebuild_search_cache();

    let app = build_app(new_test_state(catalog));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v2/songs/search?q=Burn%20Noah&mode=and")
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
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"].as_str().expect("id"), "target");
}

#[tokio::test]
async fn songs_search_mode_and_phrase_supports_unique_match() {
    let mut catalog = SongCatalog::default();

    let exact_phrase = Arc::new(SongInfo {
        id: "phrase".to_string(),
        name: "Indelible Scar".to_string(),
        composer: "Noah".to_string(),
        illustrator: "i".to_string(),
        chart_constants: chart_constants_none(),
    });
    let shuffled = Arc::new(SongInfo {
        id: "shuffled".to_string(),
        name: "Scar Indelible".to_string(),
        composer: "Noah".to_string(),
        illustrator: "i".to_string(),
        chart_constants: chart_constants_none(),
    });

    for song in [&exact_phrase, &shuffled] {
        catalog.by_id.insert(song.id.clone(), Arc::clone(song));
    }
    catalog.rebuild_search_cache();

    let app = build_app(new_test_state(catalog));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v2/songs/search?q=%22Indelible%20Scar%22&mode=and&unique=true")
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
    assert_eq!(v["id"].as_str().expect("id"), "phrase");
}

#[tokio::test]
async fn songs_search_invalid_mode_is_validation_error() {
    let mut catalog = SongCatalog::default();
    catalog.rebuild_search_cache();

    let app = build_app(new_test_state(catalog));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v2/songs/search?q=Burn&mode=xor")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request");

    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn songs_search_mode_and_uses_normalized_matching() {
    let mut catalog = SongCatalog::default();

    let target = Arc::new(SongInfo {
        id: "quo".to_string(),
        name: "君往何处 (Quo Vadis)".to_string(),
        composer: "M2U".to_string(),
        illustrator: "i".to_string(),
        chart_constants: chart_constants_none(),
    });
    let other = Arc::new(SongInfo {
        id: "other".to_string(),
        name: "Quo Start".to_string(),
        composer: "Other".to_string(),
        illustrator: "i".to_string(),
        chart_constants: chart_constants_none(),
    });

    for song in [&target, &other] {
        catalog.by_id.insert(song.id.clone(), Arc::clone(song));
    }
    catalog.rebuild_search_cache();

    let app = build_app(new_test_state(catalog));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v2/songs/search?q=QuoVadis%20M2U&mode=and")
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
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"].as_str().expect("id"), "quo");
}

#[tokio::test]
async fn songs_search_mode_or_alias_score_only_affects_own_song() {
    let mut catalog = SongCatalog::default();

    let alias_song = Arc::new(SongInfo {
        id: "alias".to_string(),
        name: "Indelible Scar".to_string(),
        composer: "Noah".to_string(),
        illustrator: "i".to_string(),
        chart_constants: chart_constants_none(),
    });
    let other_song = Arc::new(SongInfo {
        id: "other".to_string(),
        name: "Noah Theme".to_string(),
        composer: "Noah".to_string(),
        illustrator: "i".to_string(),
        chart_constants: chart_constants_none(),
    });

    catalog
        .by_id
        .insert(alias_song.id.clone(), Arc::clone(&alias_song));
    catalog
        .by_id
        .insert(other_song.id.clone(), Arc::clone(&other_song));
    catalog
        .by_nickname
        .entry("IS".to_string())
        .or_default()
        .push(Arc::clone(&alias_song));
    catalog.rebuild_search_cache();

    let app = build_app(new_test_state(catalog));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v2/songs/search?q=IS%20Noah&mode=or&limit=2")
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
    assert_eq!(items[0]["id"].as_str().expect("first id"), "alias");
    assert_eq!(items[1]["id"].as_str().expect("second id"), "other");
}

#[tokio::test]
async fn songs_search_exact_alias_is_ranked_before_name_contains() {
    let mut catalog = SongCatalog::default();

    let alias_song = Arc::new(SongInfo {
        id: "alias".to_string(),
        name: "Indelible Scar".to_string(),
        composer: "Noah".to_string(),
        illustrator: "i".to_string(),
        chart_constants: chart_constants_none(),
    });
    let contains_song = Arc::new(SongInfo {
        id: "contains".to_string(),
        name: "This Song".to_string(),
        composer: "c".to_string(),
        illustrator: "i".to_string(),
        chart_constants: chart_constants_none(),
    });

    catalog
        .by_id
        .insert(alias_song.id.clone(), Arc::clone(&alias_song));
    catalog
        .by_id
        .insert(contains_song.id.clone(), Arc::clone(&contains_song));
    catalog
        .by_nickname
        .entry("IS".to_string())
        .or_default()
        .push(Arc::clone(&alias_song));
    catalog.rebuild_search_cache();

    let app = build_app(new_test_state(catalog));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v2/songs/search?q=is&limit=2")
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
    assert_eq!(items[0]["id"].as_str().expect("first id"), "alias");
    assert_eq!(items[1]["id"].as_str().expect("second id"), "contains");
}

#[tokio::test]
async fn songs_search_unique_exact_name_ignores_weaker_contains_matches() {
    let mut catalog = SongCatalog::default();

    let exact = Arc::new(SongInfo {
        id: "stasis".to_string(),
        name: "Stasis".to_string(),
        composer: "Maozon".to_string(),
        illustrator: "i".to_string(),
        chart_constants: chart_constants_none(),
    });
    let contains_a = Arc::new(SongInfo {
        id: "chrono".to_string(),
        name: "Chronostasis".to_string(),
        composer: "c".to_string(),
        illustrator: "i".to_string(),
        chart_constants: chart_constants_none(),
    });
    let contains_b = Arc::new(SongInfo {
        id: "khro".to_string(),
        name: "Khronostasis Katharsis".to_string(),
        composer: "c".to_string(),
        illustrator: "i".to_string(),
        chart_constants: chart_constants_none(),
    });

    for song in [&exact, &contains_a, &contains_b] {
        catalog.by_id.insert(song.id.clone(), Arc::clone(song));
    }
    catalog.rebuild_search_cache();

    let app = build_app(new_test_state(catalog));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v2/songs/search?q=Stasis&unique=true")
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
    assert_eq!(v["id"].as_str().expect("id"), "stasis");
    assert_eq!(v["name"].as_str().expect("name"), "Stasis");
}

#[tokio::test]
async fn songs_search_unique_exact_alias_ignores_weaker_contains_matches() {
    let mut catalog = SongCatalog::default();

    let alias_song = Arc::new(SongInfo {
        id: "alias".to_string(),
        name: "Indelible Scar".to_string(),
        composer: "Noah".to_string(),
        illustrator: "i".to_string(),
        chart_constants: chart_constants_none(),
    });
    let contains_song = Arc::new(SongInfo {
        id: "contains".to_string(),
        name: "This Song".to_string(),
        composer: "c".to_string(),
        illustrator: "i".to_string(),
        chart_constants: chart_constants_none(),
    });

    catalog
        .by_id
        .insert(alias_song.id.clone(), Arc::clone(&alias_song));
    catalog
        .by_id
        .insert(contains_song.id.clone(), Arc::clone(&contains_song));
    catalog
        .by_nickname
        .entry("IS".to_string())
        .or_default()
        .push(Arc::clone(&alias_song));
    catalog.rebuild_search_cache();

    let app = build_app(new_test_state(catalog));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v2/songs/search?q=is&unique=true")
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
    assert_eq!(v["id"].as_str().expect("id"), "alias");
    assert_eq!(v["name"].as_str().expect("name"), "Indelible Scar");
}

#[tokio::test]
async fn songs_search_unique_normalized_alias_beats_name_prefix() {
    let mut catalog = SongCatalog::default();

    let alias_song = Arc::new(SongInfo {
        id: "burn".to_string(),
        name: "Burn".to_string(),
        composer: "c".to_string(),
        illustrator: "i".to_string(),
        chart_constants: chart_constants_none(),
    });
    let prefix_song = Arc::new(SongInfo {
        id: "prefix".to_string(),
        name: "Burn Spectrum".to_string(),
        composer: "c".to_string(),
        illustrator: "i".to_string(),
        chart_constants: chart_constants_none(),
    });

    catalog
        .by_id
        .insert(alias_song.id.clone(), Arc::clone(&alias_song));
    catalog
        .by_id
        .insert(prefix_song.id.clone(), Arc::clone(&prefix_song));
    catalog
        .by_nickname
        .entry("BurnSP".to_string())
        .or_default()
        .push(Arc::clone(&alias_song));
    catalog.rebuild_search_cache();

    let app = build_app(new_test_state(catalog));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v2/songs/search?q=burn%20sp&unique=true")
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
    assert_eq!(v["id"].as_str().expect("id"), "burn");
    assert_eq!(v["name"].as_str().expect("name"), "Burn");
}

#[tokio::test]
async fn songs_search_unique_fuzzy_alias_returns_target_when_no_regular_match_exists() {
    let mut catalog = SongCatalog::default();

    let alias_song = Arc::new(SongInfo {
        id: "burn".to_string(),
        name: "Burn".to_string(),
        composer: "c".to_string(),
        illustrator: "i".to_string(),
        chart_constants: chart_constants_none(),
    });
    let other_song = Arc::new(SongInfo {
        id: "other".to_string(),
        name: "Spectrum".to_string(),
        composer: "c".to_string(),
        illustrator: "i".to_string(),
        chart_constants: chart_constants_none(),
    });

    catalog
        .by_id
        .insert(alias_song.id.clone(), Arc::clone(&alias_song));
    catalog
        .by_id
        .insert(other_song.id.clone(), Arc::clone(&other_song));
    catalog
        .by_nickname
        .entry("BurnSP".to_string())
        .or_default()
        .push(Arc::clone(&alias_song));
    catalog.rebuild_search_cache();

    let app = build_app(new_test_state(catalog));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v2/songs/search?q=burnsx&unique=true")
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
    assert_eq!(v["id"].as_str().expect("id"), "burn");
    assert_eq!(v["name"].as_str().expect("name"), "Burn");
}

#[tokio::test]
async fn songs_search_unique_not_unique_returns_candidates_preview() {
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

    catalog.by_id.insert(a.id.clone(), Arc::clone(&a));
    catalog.by_id.insert(b.id.clone(), Arc::clone(&b));
    catalog.rebuild_search_cache();

    let app = build_app(new_test_state(catalog));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v2/songs/search?q=Alpha&unique=true")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request");

    assert_eq!(resp.status(), StatusCode::CONFLICT);

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("read body");
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");
    assert_eq!(v["code"].as_str().expect("code"), "SEARCH_NOT_UNIQUE");
    assert_eq!(v["candidatesTotal"].as_u64().expect("candidatesTotal"), 2);

    let candidates = v["candidates"].as_array().expect("candidates array");
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0]["id"].as_str().expect("id"), "a");
    assert_eq!(candidates[0]["name"].as_str().expect("name"), "Alpha");
    assert_eq!(candidates[1]["id"].as_str().expect("id"), "b");
    assert_eq!(candidates[1]["name"].as_str().expect("name"), "Alpha");
}

#[tokio::test]
async fn songs_search_unique_candidates_are_limited_but_total_is_reported() {
    let mut catalog = SongCatalog::default();

    for i in 0..30 {
        let id = format!("id-{i:03}");
        let info = Arc::new(SongInfo {
            id: id.clone(),
            name: "Alpha".to_string(),
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
                .uri("/api/v2/songs/search?q=Alpha&unique=true")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request");

    assert_eq!(resp.status(), StatusCode::CONFLICT);

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("read body");
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");
    assert_eq!(v["code"].as_str().expect("code"), "SEARCH_NOT_UNIQUE");
    assert_eq!(v["candidatesTotal"].as_u64().expect("candidatesTotal"), 30);

    let candidates = v["candidates"].as_array().expect("candidates array");
    assert_eq!(candidates.len(), 10);
    assert_eq!(candidates[0]["id"].as_str().expect("id"), "id-000");
    assert_eq!(candidates[9]["id"].as_str().expect("id"), "id-009");
}
