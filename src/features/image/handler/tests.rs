use super::{
    ImageOutputCacheSpec, ImageQueryOpts, content_type_from_fmt_code, format_code,
    parse_user_score_difficulty,
};
use axum::Json;
use axum::extract::{Query, State};
use std::sync::Arc;
use tokio::sync::Semaphore;

#[test]
fn supports_svg_format_code_and_content_type() {
    let q = ImageQueryOpts {
        format: Some("svg".to_string()),
        template: None,
        width: None,
        webp_quality: None,
        webp_lossless: None,
    };
    assert_eq!(format_code(&q), "svg");
    assert_eq!(
        content_type_from_fmt_code("svg"),
        "image/svg+xml; charset=utf-8"
    );
}

#[test]
fn output_cache_spec_normalizes_webp_cache_dimensions() {
    let q = ImageQueryOpts {
        format: Some("webp".to_string()),
        template: Some("custom".to_string()),
        width: Some(720),
        webp_quality: Some(95),
        webp_lossless: Some(false),
    };

    let spec = ImageOutputCacheSpec::from_query(&q, true);

    assert_eq!(spec.fmt_code, "webp");
    assert_eq!(spec.content_type, "image/webp");
    assert!(spec.embed_images_effective);
    assert_eq!(spec.public_illustration_base_url, None);
    assert_eq!(spec.width_code, 720);
    assert_eq!(spec.tpl_code, "custom");
    assert_eq!(spec.webp_quality_code, 95);
    assert_eq!(spec.webp_lossless_code, 0);
    assert_eq!(
        spec.bn_cache_key("u", 30, "updated", crate::features::image::Theme::Black),
        "u:bn:30:updated:b:1:custom:webp:720:95:0"
    );
    assert_eq!(
        spec.song_cache_key("u", "song-id", "updated"),
        "u:song:song-id:updated:d:1:custom:webp:720:95:0"
    );
}

#[test]
fn output_cache_spec_forces_svg_cache_dimensions() {
    // 避免测试并发初始化冲突：已初始化则忽略错误。
    let _ = crate::config::AppConfig::init_global();

    let q = ImageQueryOpts {
        format: Some("svg".to_string()),
        template: None,
        width: Some(720),
        webp_quality: Some(95),
        webp_lossless: Some(true),
    };

    let spec = ImageOutputCacheSpec::from_query(&q, true);

    assert_eq!(spec.fmt_code, "svg");
    assert_eq!(spec.content_type, "image/svg+xml; charset=utf-8");
    assert!(!spec.embed_images_effective);
    assert!(spec.public_illustration_base_url.is_some());
    assert_eq!(spec.width_code, 0);
    assert_eq!(spec.tpl_code, "legacy");
    assert_eq!(spec.webp_quality_code, 0);
    assert_eq!(spec.webp_lossless_code, 0);
    assert_eq!(
        spec.bn_cache_key("u", 0, "updated", crate::features::image::Theme::White),
        "u:bn:1:updated:w:0:legacy:svg:0:0:0"
    );
    assert_eq!(
        spec.song_cache_key("u", "song-id", "updated"),
        "u:song:song-id:updated:d:0:legacy:svg:0:0:0"
    );
}

#[test]
fn user_score_difficulty_parser_accepts_case_and_whitespace() {
    let (difficulty, label) = parse_user_score_difficulty(" in ").expect("parse IN");
    assert_eq!(difficulty, super::Difficulty::IN);
    assert_eq!(label, "IN");

    let (difficulty, label) = parse_user_score_difficulty("aT").expect("parse AT");
    assert_eq!(difficulty, super::Difficulty::AT);
    assert_eq!(label, "AT");
}

#[test]
fn user_score_difficulty_parser_rejects_invalid() {
    assert!(parse_user_score_difficulty("").is_none());
    assert!(parse_user_score_difficulty("SP").is_none());
}

#[test]
fn canonical_difficulty_parser_is_strict() {
    assert_eq!(
        super::difficulty_from_canonical_label("IN"),
        Some(super::Difficulty::IN)
    );
    assert!(super::difficulty_from_canonical_label("in").is_none());
    assert!(super::difficulty_from_canonical_label(" IN ").is_none());
}

#[test]
fn user_score_difficulty_error_keeps_message_shape() {
    let err = super::user_score_difficulty_error(1, "Song", "SP");
    match err {
        crate::error::AppError::ImageRendererError(message) => {
            assert_eq!(message, "第2条成绩难度无效或无定数: Song SP");
        }
        other => panic!("expected ImageRendererError, got {other}"),
    }
}

#[test]
fn user_score_full_combo_helper_keeps_existing_rule() {
    assert!(!super::is_user_score_full_combo(None, 99.999));
    assert!(!super::is_user_score_full_combo(Some(999_999), 99.999));
    assert!(super::is_user_score_full_combo(Some(1_000_000), 99.0));
    assert!(super::is_user_score_full_combo(None, 100.0));
}

#[test]
fn render_record_stats_helpers_keep_existing_boundaries() {
    assert_eq!(super::calculate_ap_top_3_avg(&[]), None);
    assert_eq!(super::calculate_best_27_avg(&[]), None);
    assert!(super::collect_ap_top_3_scores(&[]).is_empty());

    let records = vec![
        test_render_record("A", 100.0, 15.0),
        test_render_record("B", 99.0, 14.0),
        test_render_record("C", 100.0, 13.0),
        test_render_record("D", 100.0, 12.0),
    ];

    assert_eq!(super::calculate_ap_top_3_avg(&records), Some(40.0 / 3.0));
    assert_eq!(super::calculate_best_27_avg(&records), Some(13.5));
    let ap_scores = super::collect_ap_top_3_scores(&records);
    assert_eq!(ap_scores.len(), 3);
    assert_eq!(ap_scores[0].song_id, "A");
    assert_eq!(ap_scores[1].song_id, "C");
    assert_eq!(ap_scores[2].song_id, "D");
}

#[test]
fn push_acc_map_helper_respects_zero_limit() {
    let records = vec![test_render_record("A", 99.0, 10.0)];
    let engine_all = super::build_engine_records_from_render_records(&records);

    assert!(super::calculate_push_acc_map(&records, &engine_all, 0).is_empty());
}

#[test]
fn render_record_engine_helper_skips_invalid_difficulty() {
    let valid = test_render_record("A", 99.0, 10.0);
    let mut invalid = test_render_record("B", 98.0, 9.0);
    invalid.difficulty = "SP".to_string();

    let records = super::build_engine_records_from_render_records(&[valid, invalid]);

    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(record.song_id, "A");
    assert_eq!(record.difficulty, super::Difficulty::IN);
    assert_eq!(record.score, 1_000_000);
    assert_eq!(record.acc, 99.0);
    assert_eq!(record.rks, 10.0);
    assert_eq!(record.chart_constant, 15.0);
}

#[test]
fn game_record_engine_helper_skips_records_without_chart_constant() {
    let mut game_record = std::collections::HashMap::new();
    game_record.insert(
        "song-a".to_string(),
        vec![
            super::DifficultyRecord {
                difficulty: super::Difficulty::IN,
                score: 998_765,
                accuracy: 99.5,
                is_full_combo: true,
                chart_constant: None,
                push_acc: None,
                push_acc_hint: None,
            },
            super::DifficultyRecord {
                difficulty: super::Difficulty::AT,
                score: 987_654,
                accuracy: 98.0,
                is_full_combo: false,
                chart_constant: None,
                push_acc: None,
                push_acc_hint: None,
            },
        ],
    );

    let mut chart_constants = crate::startup::chart_loader::ChartConstantsMap::new();
    chart_constants.insert(
        "song-a".to_string(),
        crate::startup::chart_loader::ChartConstants {
            ez: None,
            hd: None,
            in_level: Some(15.2),
            at: None,
        },
    );

    let records = super::build_engine_records_from_game_record(&game_record, &chart_constants);

    assert_eq!(records.len(), 1);
    let record = &records[0];
    let chart_constant = f64::from(15.2_f32);
    assert_eq!(record.song_id, "song-a");
    assert_eq!(record.difficulty, super::Difficulty::IN);
    assert_eq!(record.score, 998_765);
    assert_eq!(record.acc, 99.5);
    assert_eq!(record.chart_constant, chart_constant);
    assert_eq!(
        record.rks,
        crate::rks_contract::engine::calculate_chart_rks(99.5, chart_constant)
    );
}

#[test]
fn song_engine_index_helper_keeps_first_matching_difficulty() {
    let records = vec![
        crate::rks_contract::engine::RksRecord {
            song_id: "other-song".to_string(),
            difficulty: super::Difficulty::IN,
            score: 999_999,
            acc: 99.9,
            rks: 16.0,
            chart_constant: 16.0,
        },
        crate::rks_contract::engine::RksRecord {
            song_id: "song-a".to_string(),
            difficulty: super::Difficulty::IN,
            score: 998_765,
            acc: 99.5,
            rks: 15.2,
            chart_constant: 15.2,
        },
        crate::rks_contract::engine::RksRecord {
            song_id: "song-a".to_string(),
            difficulty: super::Difficulty::IN,
            score: 997_654,
            acc: 99.0,
            rks: 15.0,
            chart_constant: 15.2,
        },
        crate::rks_contract::engine::RksRecord {
            song_id: "song-a".to_string(),
            difficulty: super::Difficulty::EZ,
            score: 1_000_000,
            acc: 100.0,
            rks: 5.0,
            chart_constant: 5.0,
        },
    ];

    let indices = super::find_song_engine_record_indices(&records, "song-a");

    assert_eq!(
        indices[super::difficulty_index(super::Difficulty::EZ)],
        Some(3)
    );
    assert_eq!(
        indices[super::difficulty_index(super::Difficulty::HD)],
        None
    );
    assert_eq!(
        indices[super::difficulty_index(super::Difficulty::IN)],
        Some(1)
    );
    assert_eq!(
        indices[super::difficulty_index(super::Difficulty::AT)],
        None
    );
}

#[test]
fn song_difficulty_record_index_helper_keeps_first_matching_record() {
    let records = vec![
        test_difficulty_record(super::Difficulty::IN, 998_765),
        test_difficulty_record(super::Difficulty::EZ, 1_000_000),
        test_difficulty_record(super::Difficulty::IN, 997_654),
    ];

    let indices = super::index_song_difficulty_records(&records);

    assert_eq!(
        indices[super::difficulty_index(super::Difficulty::EZ)].map(|record| record.score),
        Some(1_000_000)
    );
    assert_eq!(
        indices[super::difficulty_index(super::Difficulty::HD)].map(|record| record.score),
        None
    );
    assert_eq!(
        indices[super::difficulty_index(super::Difficulty::IN)].map(|record| record.score),
        Some(998_765)
    );
    assert_eq!(
        indices[super::difficulty_index(super::Difficulty::AT)].map(|record| record.score),
        None
    );
}

#[test]
fn sort_render_records_by_rks_desc_keeps_existing_order_rule() {
    let mut records = vec![
        test_render_record("A", 99.0, 10.0),
        test_render_record("B", 99.0, 12.0),
        test_render_record("C", 99.0, 9.0),
    ];

    super::sort_render_records_by_rks_desc(&mut records);

    let song_ids: Vec<_> = records
        .iter()
        .map(|record| record.song_id.as_str())
        .collect();
    assert_eq!(song_ids, vec!["B", "A", "C"]);
}

#[test]
fn sort_engine_records_by_rks_desc_keeps_existing_order_rule() {
    let mut records = vec![
        test_engine_record("A", super::Difficulty::IN, 10.0),
        test_engine_record("B", super::Difficulty::HD, 12.0),
        test_engine_record("C", super::Difficulty::EZ, 9.0),
    ];

    super::sort_engine_records_by_rks_desc(&mut records);

    let song_ids: Vec<_> = records
        .iter()
        .map(|record| record.song_id.as_str())
        .collect();
    assert_eq!(song_ids, vec!["B", "A", "C"]);
}

#[test]
fn parse_update_time_or_now_converts_rfc3339_to_utc() {
    let parsed = super::parse_update_time_or_now(Some("2026-06-03T08:00:00+08:00"));

    assert_eq!(parsed.to_rfc3339(), "2026-06-03T00:00:00+00:00");
}

#[test]
fn parse_challenge_rank_keeps_color_mapping() {
    assert_eq!(
        super::parse_challenge_rank(401),
        Some(("Gold".to_string(), "01".to_string()))
    );
    assert_eq!(super::parse_challenge_rank(0), None);
    assert_eq!(super::parse_challenge_rank(601), None);
}

#[test]
fn format_data_string_preserves_existing_unit_order() {
    assert_eq!(super::format_data_string(&[0, 0, 0, 0, 0]), None);
    assert_eq!(
        super::format_data_string(&[1, 2, 0, 4, 9]),
        Some("Data: 4 TB, 2 MB, 1 KB".to_string())
    );
}

#[test]
fn save_updated_cache_version_preserves_none_fallback() {
    assert_eq!(super::save_updated_cache_version(None), "none");
    assert_eq!(
        super::save_updated_cache_version(Some("updated")),
        "updated"
    );
}

#[test]
fn image_footer_text_uses_global_branding_footer() {
    // 避免测试并发初始化冲突：已初始化则忽略错误。
    let _ = crate::config::AppConfig::init_global();

    assert_eq!(
        super::image_footer_text(),
        Some(
            crate::config::AppConfig::global()
                .branding
                .footer_text
                .clone()
        )
    );
}

#[test]
fn image_cache_enabled_uses_global_image_config() {
    // 避免测试并发初始化冲突：已初始化则忽略错误。
    let _ = crate::config::AppConfig::init_global();

    assert_eq!(
        super::image_cache_enabled(),
        crate::config::AppConfig::global().image.cache_enabled
    );
}

#[test]
fn image_user_identity_helper_keeps_empty_identity_boundary() {
    // 避免测试并发初始化冲突：已初始化则忽略错误。
    let _ = crate::config::AppConfig::init_global();

    let auth = crate::auth_contract::UnifiedSaveRequest {
        session_token: None,
        external_credentials: None,
        taptap_version: None,
    };
    let identity =
        super::derive_image_user_identity(&auth, &crate::session_auth::BearerAuthState::Absent)
            .expect("empty auth and absent bearer should be accepted");

    assert_eq!(identity, (None, None));
}

fn test_render_record(song_id: &str, acc: f64, rks: f64) -> crate::features::image::RenderRecord {
    crate::features::image::RenderRecord {
        song_id: song_id.to_string(),
        song_name: song_id.to_string(),
        difficulty: "IN".to_string(),
        score: Some(1_000_000.0),
        acc,
        rks,
        difficulty_value: 15.0,
        is_fc: acc >= 100.0,
    }
}

fn test_difficulty_record(difficulty: super::Difficulty, score: u32) -> super::DifficultyRecord {
    super::DifficultyRecord {
        difficulty,
        score,
        accuracy: 99.5,
        is_full_combo: false,
        chart_constant: None,
        push_acc: None,
        push_acc_hint: None,
    }
}

fn test_engine_record(
    song_id: &str,
    difficulty: super::Difficulty,
    rks: f64,
) -> crate::rks_contract::engine::RksRecord {
    crate::rks_contract::engine::RksRecord {
        song_id: song_id.to_string(),
        difficulty,
        score: 1_000_000,
        acc: 99.5,
        rks,
        chart_constant: 15.0,
    }
}

#[test]
fn topn_drain_is_equivalent_to_clone_take() {
    use crate::features::image::RenderRecord;

    let mut all = vec![
        RenderRecord {
            song_id: "A".into(),
            song_name: "SongA".into(),
            difficulty: "IN".into(),
            score: Some(1_000_000.0),
            acc: 99.0,
            rks: 10.0,
            difficulty_value: 15.0,
            is_fc: false,
        },
        RenderRecord {
            song_id: "B".into(),
            song_name: "SongB".into(),
            difficulty: "HD".into(),
            score: Some(900_000.0),
            acc: 98.0,
            rks: 12.0,
            difficulty_value: 14.0,
            is_fc: false,
        },
        RenderRecord {
            song_id: "C".into(),
            song_name: "SongC".into(),
            difficulty: "EZ".into(),
            score: Some(1_000_000.0),
            acc: 100.0,
            rks: 15.0,
            difficulty_value: 9.0,
            is_fc: true,
        },
        RenderRecord {
            song_id: "D".into(),
            song_name: "SongD".into(),
            difficulty: "AT".into(),
            score: Some(800_000.0),
            acc: 97.0,
            rks: 9.0,
            difficulty_value: 16.0,
            is_fc: false,
        },
    ];

    let n = 3usize;

    let mut all_old = all.clone();
    all_old.sort_by(|a, b| {
        b.rks
            .partial_cmp(&a.rks)
            .unwrap_or(core::cmp::Ordering::Equal)
    });
    let top_old: Vec<RenderRecord> = all_old.iter().take(n).cloned().collect();

    all.sort_by(|a, b| {
        b.rks
            .partial_cmp(&a.rks)
            .unwrap_or(core::cmp::Ordering::Equal)
    });
    let top_len = n.min(all.len());
    let top_new: Vec<RenderRecord> = all.drain(..top_len).collect();

    assert_eq!(top_new.len(), top_old.len());
    for (a, b) in top_new.iter().zip(top_old.iter()) {
        assert_eq!(a.song_id, b.song_id);
        assert_eq!(a.difficulty, b.difficulty);
        assert_eq!(a.rks, b.rks);
        assert_eq!(a.acc, b.acc);
    }
}

fn dummy_state() -> crate::state::AppState {
    use axum::body::Bytes;
    use moka::future::Cache;

    let chart_constants = Arc::new(crate::startup::chart_loader::ChartConstantsMap::new());
    let song_catalog = Arc::new(crate::song_contract::SongCatalog::default());

    let taptap_client = Arc::new(
        crate::auth_services::TapTapClient::new(&crate::config::TapTapMultiConfig::default())
            .expect("init TapTapClient"),
    );
    let qrcode_service = Arc::new(crate::auth_services::QrCodeService::new());

    crate::state::AppState {
        chart_constants,
        song_catalog,
        taptap_client,
        qrcode_service,
        stats: None,
        stats_storage: None,
        render_semaphore: Arc::new(Semaphore::new(1)),
        bn_image_cache: Cache::<String, Bytes>::builder().max_capacity(1).build(),
        song_image_cache: Cache::<String, Bytes>::builder().max_capacity(1).build(),
    }
}

#[tokio::test]
async fn image_ban_check_noops_without_storage_or_hash() {
    let state = dummy_state();

    super::ensure_image_user_not_banned(&state, None)
        .await
        .expect("missing user hash should not require storage");
    super::ensure_image_user_not_banned(&state, Some("user-hash"))
        .await
        .expect("missing storage should be a no-op");
}

#[tokio::test]
async fn user_bn_rejects_scores_over_limit() {
    // 避免测试并发初始化冲突：已初始化则忽略错误。
    let _ = crate::config::AppConfig::init_global();

    let cfg = crate::config::AppConfig::global();
    let max_scores = super::usize_from_u32(cfg.image.max_user_scores);
    assert!(max_scores > 0, "测试需要 max_user_scores > 0");

    let over = max_scores + 1;
    let mut scores = Vec::with_capacity(over);
    for _ in 0..over {
        scores.push(crate::features::image::types::UserScoreItem {
            song: "dummy".into(),
            difficulty: "IN".into(),
            acc: 99.0,
            score: None,
        });
    }

    let state = dummy_state();
    let q = ImageQueryOpts::default();
    let req = crate::features::image::types::RenderUserBnRequest {
        theme: crate::features::image::Theme::default(),
        nickname: None,
        unlock_password: None,
        scores,
    };

    let res = super::render_bn_user(State(state), Query(q), Json(req)).await;
    match res {
        Err(crate::error::AppError::Validation(msg)) => {
            assert!(msg.contains("scores 条数超过上限"), "msg={msg}");
        }
        Err(e) => panic!("expected Validation error, got Err: {e}"),
        Ok(_) => panic!("expected Validation error, got Ok"),
    }
}
