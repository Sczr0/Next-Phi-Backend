use super::urls::{
    ExternalIllustrationDirMode, build_remote_illustration_url_with_options,
    remote_illustration_dir_for_category, to_public_url_for_base, to_somnia_public_url_for_base,
};
use super::{
    PlayerStats, RenderRecord, SongRenderData, Theme, generate_song_svg_string,
    generate_svg_string, render_svg_unified,
};
use chrono::Utc;
use std::fs;
use std::sync::OnceLock;

fn ensure_config_inited() {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        // 注意：全局配置只允许初始化一次；为避免与其它测试（如管理员令牌测试）发生顺序依赖，
        // 在这里提前设置一个确定的默认管理员令牌集合。
        unsafe {
            std::env::set_var("APP_LEADERBOARD_ADMIN_TOKENS", "t1,t2");
        }
        let _ = crate::config::AppConfig::init_global();
    });
}

#[test]
fn maps_local_path_to_public_url() {
    let base_dir = std::env::temp_dir().join(format!("phi-backend-test-{}", uuid::Uuid::new_v4()));
    let ill_dir = base_dir.join("ill");
    fs::create_dir_all(&ill_dir).unwrap();
    let file_path = ill_dir.join("123.png");
    fs::write(&file_path, b"test").unwrap();

    let url = to_public_url_for_base(&file_path, &base_dir, "/_ill").unwrap();
    assert_eq!(url, "/_ill/ill/123.png");

    let _ = fs::remove_dir_all(&base_dir);
}

#[test]
fn returns_none_when_not_under_base_dir() {
    let base_dir = std::env::temp_dir().join(format!("phi-backend-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&base_dir).unwrap();
    let other_dir =
        std::env::temp_dir().join(format!("phi-backend-test-other-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&other_dir).unwrap();
    let file_path = other_dir.join("ill").join("123.png");
    fs::create_dir_all(file_path.parent().unwrap()).unwrap();
    fs::write(&file_path, b"test").unwrap();

    let url = to_public_url_for_base(&file_path, &base_dir, "/_ill");
    assert!(url.is_none());

    let _ = fs::remove_dir_all(&base_dir);
    let _ = fs::remove_dir_all(&other_dir);
}

#[test]
fn maps_local_path_to_somnia_url_for_external_base() {
    ensure_config_inited();
    let base_dir = std::env::temp_dir().join(format!("phi-backend-test-{}", uuid::Uuid::new_v4()));
    let ill_low_dir = base_dir.join("illLow");
    fs::create_dir_all(&ill_low_dir).unwrap();
    let file_path = ill_low_dir.join("A B.png");
    fs::write(&file_path, b"test").unwrap();

    let url = to_somnia_public_url_for_base(&file_path, &base_dir, "https://example.com").unwrap();
    assert_eq!(url, "https://example.com/illustrationLowRes/A%20B.png");

    let _ = fs::remove_dir_all(&base_dir);
}

#[test]
fn maps_category_to_lilith_remote_dir() {
    assert_eq!(
        remote_illustration_dir_for_category("ill", ExternalIllustrationDirMode::Lilith),
        Some("ill")
    );
    assert_eq!(
        remote_illustration_dir_for_category("illLow", ExternalIllustrationDirMode::Lilith),
        Some("illLow")
    );
    assert_eq!(
        remote_illustration_dir_for_category("illBlur", ExternalIllustrationDirMode::Lilith),
        Some("illBlur")
    );
}

#[test]
fn builds_remote_url_with_lilith_webp() {
    ensure_config_inited();
    let url = build_remote_illustration_url_with_options(
        "https://example.com/lilith",
        "A B",
        ExternalIllustrationDirMode::Lilith,
        "webp",
        false,
    );
    assert_eq!(url, "https://example.com/lilith/ill/A%20B.webp");

    let low_url = build_remote_illustration_url_with_options(
        "https://example.com/lilith",
        "A B",
        ExternalIllustrationDirMode::Lilith,
        "avif",
        true,
    );
    assert_eq!(low_url, "https://example.com/lilith/illLow/A%20B.avif");
}

#[test]
fn generate_svg_uses_remote_cover_url_when_base_provided() {
    ensure_config_inited();
    let record = RenderRecord {
        song_id: "TEST REMOTE 123".to_string(),
        song_name: "Remote".to_string(),
        difficulty: "IN".to_string(),
        score: Some(1_000_000.0),
        acc: 99.0,
        rks: 0.0,
        difficulty_value: 10.0,
        is_fc: false,
    };
    let stats = PlayerStats {
        ap_top_3_avg: None,
        best_27_avg: None,
        real_rks: None,
        player_name: Some("Tester".to_string()),
        update_time: Utc::now(),
        n: 1,
        ap_top_3_scores: vec![],
        challenge_rank: None,
        data_string: None,
        custom_footer_text: None,
        is_user_generated: false,
    };

    let svg = generate_svg_string::<std::collections::hash_map::RandomState>(
        &[record],
        &stats,
        None,
        &Theme::default(),
        false,
        Some("https://example.com"),
        None,
    )
    .unwrap();

    assert!(svg.contains("https://example.com/illustrationLowRes/TEST%20REMOTE%20123.png"));
    assert!(!svg.contains("data:image/"));
}

#[test]
fn generate_song_svg_uses_remote_illust_when_missing_path() {
    ensure_config_inited();
    let data = SongRenderData {
        song_name: "RemoteSong".to_string(),
        song_id: "SONG REMOTE 456".to_string(),
        player_name: Some("Tester".to_string()),
        update_time: Utc::now(),
        difficulty_scores: std::collections::HashMap::default(),
        illustration_path: None,
        custom_footer_text: None,
    };

    let svg = generate_song_svg_string(&data, false, Some("https://example.com"), None).unwrap();
    assert!(svg.contains("https://example.com/illustration/SONG%20REMOTE%20456.png"));
    assert!(!svg.contains("data:image/"));
}

#[test]
fn generate_bn_svg_renders_with_external_template() {
    ensure_config_inited();
    let record = RenderRecord {
        song_id: "TEMPLATE_TEST".to_string(),
        song_name: "TemplateSong".to_string(),
        difficulty: "IN".to_string(),
        score: Some(1_000_000.0),
        acc: 99.5,
        rks: 12.34,
        difficulty_value: 15.8,
        is_fc: true,
    };
    let stats = PlayerStats {
        ap_top_3_avg: None,
        best_27_avg: Some(12.3456),
        real_rks: Some(12.345_678),
        player_name: Some("Tester".to_string()),
        update_time: Utc::now(),
        n: 1,
        ap_top_3_scores: vec![],
        challenge_rank: None,
        data_string: None,
        custom_footer_text: None,
        is_user_generated: false,
    };
    let svg = generate_svg_string::<std::collections::hash_map::RandomState>(
        &[record],
        &stats,
        None,
        &Theme::default(),
        false,
        None,
        Some("default"),
    )
    .unwrap();
    assert!(svg.contains("<svg"));
    assert!(svg.contains("id=\"main-cards\""));
}

#[test]
fn generate_bn_svg_renders_with_neo_template() {
    ensure_config_inited();
    let record = RenderRecord {
        song_id: "TEMPLATE_TEST".to_string(),
        song_name: "TemplateSong".to_string(),
        difficulty: "IN".to_string(),
        score: Some(1_000_000.0),
        acc: 99.5,
        rks: 12.34,
        difficulty_value: 15.8,
        is_fc: true,
    };
    let stats = PlayerStats {
        ap_top_3_avg: None,
        best_27_avg: Some(12.3456),
        real_rks: Some(12.345_678),
        player_name: Some("Tester".to_string()),
        update_time: Utc::now(),
        n: 1,
        ap_top_3_scores: vec![],
        challenge_rank: None,
        data_string: None,
        custom_footer_text: None,
        is_user_generated: false,
    };
    let svg = generate_svg_string::<std::collections::hash_map::RandomState>(
        &[record],
        &stats,
        None,
        &Theme::default(),
        false,
        None,
        Some("neo"),
    )
    .unwrap();
    assert!(svg.contains("<svg"));
    assert!(svg.contains("id=\"main-cards\""));
}

#[test]
fn generate_bn_svg_renders_with_firstlook_template() {
    ensure_config_inited();
    let record = RenderRecord {
        song_id: "TEMPLATE_TEST".to_string(),
        song_name: "TemplateSong".to_string(),
        difficulty: "IN".to_string(),
        score: Some(1_000_000.0),
        acc: 99.5,
        rks: 12.34,
        difficulty_value: 15.8,
        is_fc: true,
    };
    let stats = PlayerStats {
        ap_top_3_avg: None,
        best_27_avg: Some(12.3456),
        real_rks: Some(12.345_678),
        player_name: Some("Tester".to_string()),
        update_time: Utc::now(),
        n: 1,
        ap_top_3_scores: vec![record.clone()],
        challenge_rank: None,
        data_string: None,
        custom_footer_text: None,
        is_user_generated: false,
    };
    let svg = generate_svg_string::<std::collections::hash_map::RandomState>(
        &[record],
        &stats,
        None,
        &Theme::default(),
        false,
        None,
        Some("firstlook"),
    )
    .unwrap();
    assert!(svg.contains("<svg"));
    assert!(svg.contains("id=\"dashboard-header\""));
    assert!(svg.contains("id=\"champion-wall\""));
    assert!(svg.contains("id=\"main-cards\""));
}

#[test]
fn generate_song_svg_renders_with_external_template() {
    ensure_config_inited();
    let data = SongRenderData {
        song_name: "TemplateSong".to_string(),
        song_id: "TEMPLATE_SONG_ID".to_string(),
        player_name: Some("Tester".to_string()),
        update_time: Utc::now(),
        difficulty_scores: std::collections::HashMap::default(),
        illustration_path: None,
        custom_footer_text: Some("Footer".to_string()),
    };
    let svg = generate_song_svg_string(&data, false, None, Some("default")).unwrap();
    assert!(svg.contains("<svg"));
    assert!(svg.contains("difficulty-card"));
}

#[test]
fn webp_encoding_respects_quality_and_lossless() {
    ensure_config_inited();

    let svg =
        r##"<svg width="128" height="128" viewBox="0 0 128 128" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="g" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#ff0000"/>
      <stop offset="50%" stop-color="#00ff00"/>
      <stop offset="100%" stop-color="#0000ff"/>
    </linearGradient>
  </defs>
  <rect x="0" y="0" width="128" height="128" fill="url(#g)"/>
  <circle cx="64" cy="64" r="42" fill="rgba(255,255,255,0.35)"/>
</svg>"##
            .to_string();

    let (q20, ct20) =
        render_svg_unified(&svg, false, Some("webp"), Some(64), Some(20), Some(false)).unwrap();
    assert_eq!(ct20, "image/webp");

    let (q90, _ct90) =
        render_svg_unified(&svg, false, Some("webp"), Some(64), Some(90), Some(false)).unwrap();
    assert_ne!(q20, q90);

    let img20 = image::load_from_memory(&q20).unwrap();
    let img90 = image::load_from_memory(&q90).unwrap();
    assert_eq!((img20.width(), img20.height()), (64, 64));
    assert_eq!((img90.width(), img90.height()), (64, 64));

    let (lossless_q20, _ct1) =
        render_svg_unified(&svg, false, Some("webp"), Some(64), Some(20), Some(true)).unwrap();
    let (lossless_q90, _ct2) =
        render_svg_unified(&svg, false, Some("webp"), Some(64), Some(90), Some(true)).unwrap();

    let img_l1 = image::load_from_memory(&lossless_q20).unwrap().to_rgba8();
    let img_l2 = image::load_from_memory(&lossless_q90).unwrap().to_rgba8();
    assert_eq!(img_l1.dimensions(), (64, 64));
    assert_eq!(img_l2.dimensions(), (64, 64));
    assert_eq!(img_l1.as_raw(), img_l2.as_raw());
}
