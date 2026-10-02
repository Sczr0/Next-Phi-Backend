//! B27 渲染基准（CodSpeed criterion 兼容层）：SVG 生成 + PNG 光栅化。
//!
//! - `bn_svg_generate_27`：minijinja 之外的内置手写 SVG 拼装（BnLayout/卡片/文本），
//!   27 条成绩，纯 CPU 字符串构建；
//! - `bn_png_rasterize_27`：resvg 光栅化 + 封面/背景图解码 + PNG 编码——B27 管线
//!   的最大成本项，输入 SVG 在 setup 阶段固定，迭代间完全确定。
//!
//! 本地运行：`cargo bench -p impl-render --bench bn_render`
//! （注意：不带 `--bench` 具名选择时，`cargo bench` 会先跑 lib 单测，
//! criterion 专属参数会被 libtest 拒收；具名选择只跑本基准。）
//! CodSpeed：`cargo codspeed run --workspace`（见 .github/workflows/codspeed.yml）
//!
//! 环境说明：
//! - 渲染管线按“工作区根”解析 `config.example.toml` 与 `resources/fonts`（git 跟踪）；
//! - 曲绘不在 git（`.gitignore` 排除整个曲绘仓库，CI 检出后为空目录），基准使用
//!   `benches/fixtures/` 下的小型降采样 fixture（tools/gen-render-bench-fixtures.py
//!   生成，512x270，约为真实曲绘 1/16 像素量）。配置的环境变量覆盖以 `_` 为层级
//!   分隔符，`APP_RESOURCES_BASE_PATH` 会被解析为 `resources.base.path` 而非
//!   `resources.base_path`，无法用于注入；因此在临时目录生成一份 `config.toml`
//!   （由 `config.example.toml` 派生，`illustration_folder` 指向 fixture 的绝对路径，
//!   `base_path` 指向工作区 `resources` 以命中 git 跟踪的字体），并切换 CWD 到该目录；
//! - 随机背景唯一化：fixture 的 illBlur 只放 1 张图，`select_random_background`
//!   的随机选择退化为确定值。

// bench 断言与构造惯例同集成测试（tests/*.rs 的文件级豁免先例；
// doc_markdown：中文注释+代码词汇混排，同 workspace 级豁免理由）。
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::doc_markdown,
    clippy::items_after_statements,
    clippy::similar_names,
    clippy::unwrap_used,
    clippy::expect_used
)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Once;

use chrono::{TimeZone, Utc};
use criterion::{Criterion, black_box, criterion_group, criterion_main};

use impl_render::config::AppConfig;
use impl_render::features::image::Theme;
use impl_render::renderer::{
    PlayerStats, RenderRecord, generate_svg_string, get_cover_metadata_map, render_svg_to_png,
};
use impl_render::rks_contract::engine::PushAccHint;

/// B27 成绩条数
const RECORD_COUNT: usize = 27;

static INIT: Once = Once::new();

/// 初始化渲染环境（每进程一次）：
/// 1. 在临时目录生成 bench 专用 `config.toml`（资源路径全部为绝对路径），并切换 CWD
///    到该目录——`AppConfig` 只从 CWD 读取 `config.toml`/`config.example.toml`；
/// 2. 曲绘指向仓库内 fixture，字体沿用工作区 `resources/fonts`；
/// 3. 初始化全局配置并预热曲绘索引，避免一次性成本计入首个迭代。
fn init_render_env() {
    INIT.call_once(|| {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let ws_root = manifest
            .join("../..")
            .canonicalize()
            .expect("工作区根不存在");
        let fixture_ill = manifest.join("benches/fixtures/resources/ill");
        assert!(
            fixture_ill.is_dir(),
            "曲绘 fixture 目录缺失：{}",
            fixture_ill.display()
        );

        let example = std::fs::read_to_string(ws_root.join("config.example.toml"))
            .expect("读取工作区根的 config.example.toml 失败");
        let toml_str = |p: &std::path::Path| format!("{:?}", p.to_string_lossy());
        let config = example
            .lines()
            .map(|line| match line.split('=').next().map(str::trim) {
                Some("base_path") => {
                    format!("base_path = {}", toml_str(&ws_root.join("resources")))
                }
                Some("illustration_folder") => {
                    format!("illustration_folder = {}", toml_str(&fixture_ill))
                }
                Some("info_path") => format!("info_path = {}", toml_str(&ws_root.join("info"))),
                _ => line.to_string(),
            })
            .collect::<Vec<_>>()
            .join("\n");

        let run_dir = std::env::temp_dir().join("phi-backend-bn-render-bench");
        std::fs::create_dir_all(&run_dir).expect("创建 bench 临时目录失败");
        std::fs::write(run_dir.join("config.toml"), config).expect("写入 bench config.toml 失败");
        std::env::set_current_dir(&run_dir).expect("切换 CWD 到 bench 临时目录失败");

        AppConfig::init_global().expect("配置初始化失败");

        // 预热曲绘索引（目录扫描）并自检 fixture 生效
        let covers = get_cover_metadata_map();
        assert!(
            !covers.is_empty(),
            "曲绘 fixture 未加载：检查 crates/impl-render/benches/fixtures/"
        );
    });
}

/// 27 条合成成绩。song_id 取自 fixture 曲绘元数据的文件名 stem（封面可解析），
/// 数值确定性生成并按 RKS 降序，贴近真实 B27 形态（前 3 条满 ACC 进 AP 区）。
fn bench_records() -> Vec<RenderRecord> {
    let covers = get_cover_metadata_map();
    let mut song_ids: Vec<&String> = covers.keys().collect();
    song_ids.sort();
    assert!(
        song_ids.len() >= 8,
        "fixture 曲绘不足（当前 {} 张）",
        song_ids.len()
    );

    const DIFFICULTIES: [&str; 4] = ["EZ", "HD", "IN", "AT"];
    (0..RECORD_COUNT)
        .map(|i| RenderRecord {
            song_id: song_ids[i % song_ids.len()].clone(),
            song_name: format!("基准曲{i:02}·Bench"),
            difficulty: DIFFICULTIES[i % 4].to_string(),
            score: Some(950_000.0 + (i % 50) as f64 * 1000.0),
            acc: if i < 3 {
                100.0
            } else {
                95.0 + (i % 40) as f64 * 0.1
            },
            rks: 16.5 - i as f64 * 0.06,
            difficulty_value: 10.0 + (i % 60) as f64 / 10.0,
            is_fc: i % 3 == 0,
        })
        .collect()
}

/// 固定 update_time 的玩家统计（确定性输入；不依赖运行时时钟）
fn bench_stats(records: &[RenderRecord]) -> PlayerStats {
    PlayerStats {
        ap_top_3_avg: Some(16.4321),
        best_27_avg: Some(15.4321),
        real_rks: Some(15.4321),
        player_name: Some("CodSpeed基准用户".to_string()),
        update_time: Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap(),
        n: RECORD_COUNT as u32,
        ap_top_3_scores: records[0..3].to_vec(),
        challenge_rank: None,
        data_string: None,
        custom_footer_text: Some("Powered by benchmark".to_string()),
        disclaimer_text: Some(
            "本项目为非官方玩家项目，与鸽游网络及《Phigros》官方无授权或合作关系。".to_string(),
        ),
        is_user_generated: false,
    }
}

fn bench_svg_generation(c: &mut Criterion) {
    init_render_env();
    let records = bench_records();
    let stats = bench_stats(&records);
    let theme = Theme::default();
    // 显式定型 S=RandomState 的空推分表（生产按需传入，此处不测该渲染分支）
    let no_push: Option<&HashMap<String, PushAccHint>> = None;

    // 自检：27 张卡片封面 + 1 张背景的 <image> 标签齐全（fixture 未加载则此处失败）
    let svg = generate_svg_string(&records, &stats, no_push, &theme, false, None, None)
        .expect("SVG 自检失败");
    let image_tags = svg.matches("<image").count();
    assert!(
        image_tags > RECORD_COUNT,
        "SVG image 标签仅 {image_tags} 个（预期 > {RECORD_COUNT}）：曲绘 fixture 未生效"
    );

    c.bench_function("bn_svg_generate_27", |b| {
        b.iter(|| {
            generate_svg_string(
                black_box(&records),
                black_box(&stats),
                no_push,
                black_box(&theme),
                false,
                None,
                None,
            )
            .expect("SVG 生成失败")
        });
    });
}

fn bench_png_rasterization(c: &mut Criterion) {
    init_render_env();
    let records = bench_records();
    let stats = bench_stats(&records);
    let theme = Theme::default();
    let no_push: Option<&HashMap<String, PushAccHint>> = None;

    // setup 阶段固定 SVG 输入：光栅化迭代的输入完全确定（背景已唯一化为单张 fixture）
    let svg = generate_svg_string(&records, &stats, no_push, &theme, false, None, None)
        .expect("SVG 自检失败");

    // 自检：光栅化产物非平凡（字体/封面解码路径正常）
    let png = render_svg_to_png(&svg, false).expect("PNG 自检失败");
    assert!(
        png.len() > 30 * 1024,
        "PNG 产物异常偏小：{} 字节",
        png.len()
    );

    c.bench_function("bn_png_rasterize_27", |b| {
        b.iter(|| render_svg_to_png(black_box(&svg), false).expect("光栅化失败"));
    });
}

criterion_group!(benches, bench_svg_generation, bench_png_rasterization);
criterion_main!(benches);
