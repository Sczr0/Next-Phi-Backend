/// B27图片生成性能测试
///
/// 使用方式:
/// 1. 设置环境变量 PHI_SESSION_TOKEN="你的session token"
/// 2. 运行测试: cargo test --test b27_performance_test -- --nocapture --ignored
///
/// 输出:
/// - tests/output/b27.png: 生成的B27图片
/// - tests/output/flamegraph.svg: 性能火焰图（仅 Unix/Linux/macOS）
/// - tests/output/performance.txt: 性能统计报告
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

#[cfg(unix)]
use pprof::ProfilerGuardBuilder;

#[tokio::test]
#[ignore] // 需要显式运行
async fn test_b27_generation_with_flamegraph() {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_env_filter("phi_backend=debug")
        .init();

    // 从环境变量读取 session token
    let session_token =
        std::env::var("PHI_SESSION_TOKEN").expect("请设置环境变量 PHI_SESSION_TOKEN");

    println!("========================================");
    println!("B27 图片生成性能测试");
    println!("========================================\n");

    // 创建输出目录
    let output_dir = PathBuf::from("tests/output");
    fs::create_dir_all(&output_dir).expect("创建输出目录失败");

    // 启动性能分析（仅 Unix 系统）
    #[cfg(unix)]
    let _guard = {
        println!("启动性能分析器...");
        ProfilerGuardBuilder::default()
            .frequency(1000) // 采样频率 1000Hz
            .blocklist(&["libc", "libgcc", "pthread", "vdso"])
            .build()
            .expect("启动性能分析器失败")
    };

    #[cfg(not(unix))]
    {
        println!("注意: 火焰图生成仅支持 Unix/Linux/macOS 系统");
    }

    // 开始测试
    let start = std::time::Instant::now();

    // 用于收集性能报告
    let mut performance_report = Vec::new();
    performance_report.push("B27 图片生成性能测试报告".to_string());
    performance_report.push("=".repeat(50));
    performance_report.push(format!(
        "测试时间: {}",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S")
    ));
    performance_report.push(format!("操作系统: {}", std::env::consts::OS));
    performance_report.push(format!("架构: {}", std::env::consts::ARCH));
    performance_report.push("=".repeat(50));
    performance_report.push(String::new());

    println!("阶段 1: 初始化配置...");
    let phase1_start = std::time::Instant::now();

    // 初始化配置
    phi_backend::config::AppConfig::init_global().expect("配置初始化失败");
    let config = phi_backend::config::AppConfig::global();

    let phase1_elapsed = phase1_start.elapsed();
    println!("  耗时: {phase1_elapsed:?}\n");
    performance_report.push(format!("阶段 1: 初始化配置 - {phase1_elapsed:?}"));

    println!("阶段 2: 加载资源文件...");
    let phase2_start = std::time::Instant::now();

    // 加载 difficulty.csv
    let info_dir = config.info_path();
    let csv_path = info_dir.join("difficulty.csv");
    let chart_map = phi_backend::startup::chart_loader::load_chart_constants(&csv_path)
        .expect("加载 difficulty.csv 失败");

    // 加载歌曲目录
    let song_catalog =
        phi_backend::startup::song_loader::load_song_catalog(&info_dir).expect("加载歌曲目录失败");

    let phase2_elapsed = phase2_start.elapsed();
    println!("  加载了 {} 首歌曲", song_catalog.by_id.len());
    println!("  耗时: {phase2_elapsed:?}\n");
    performance_report.push(format!(
        "阶段 2: 加载资源文件 - {phase2_elapsed:?} (加载 {} 首歌曲)",
        song_catalog.by_id.len()
    ));

    println!("阶段 3: 获取并解密存档...");
    let phase3_start = std::time::Instant::now();

    // 获取存档
    use phi_backend::features::save::provider::{SaveSource, get_decrypted_save};
    let source = SaveSource::official(session_token.clone());
    let taptap_config = &config.taptap;
    let version = None; // 使用默认版本
    let parsed = get_decrypted_save(source, &chart_map, taptap_config, version)
        .await
        .expect("获取存档失败");

    let phase3_elapsed = phase3_start.elapsed();
    println!("  解析了 {} 首歌曲的成绩", parsed.game_record.len());
    println!("  耗时: {phase3_elapsed:?}\n");
    performance_report.push(format!(
        "阶段 3: 获取并解密存档 - {phase3_elapsed:?} (解析 {} 首歌曲)",
        parsed.game_record.len()
    ));

    println!("阶段 4: 计算 RKS 并排序...");
    let phase4_start = std::time::Instant::now();

    // 计算所有成绩的 RKS
    use phi_backend::features::image::RenderRecord;
    use phi_backend::features::save::models::Difficulty;
    use std::collections::HashMap;

    let mut all_records: Vec<RenderRecord> = Vec::new();
    for (song_id, diffs) in parsed.game_record.iter() {
        let chart = chart_map.get(song_id);
        let name = song_catalog
            .by_id
            .get(song_id)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| song_id.clone());

        for rec in diffs {
            let (dv_opt, diff_str) = match rec.difficulty {
                Difficulty::EZ => (chart.and_then(|c| c.ez).map(|v| v as f64), "EZ"),
                Difficulty::HD => (chart.and_then(|c| c.hd).map(|v| v as f64), "HD"),
                Difficulty::IN => (chart.and_then(|c| c.in_level).map(|v| v as f64), "IN"),
                Difficulty::AT => (chart.and_then(|c| c.at).map(|v| v as f64), "AT"),
            };
            let Some(dv) = dv_opt else { continue };

            let mut acc_percent = rec.accuracy as f64;
            if acc_percent <= 1.5 {
                acc_percent *= 100.0;
            }
            let rks = phi_backend::features::rks::engine::calculate_chart_rks(acc_percent, dv);

            all_records.push(RenderRecord {
                song_id: song_id.clone(),
                song_name: name.clone(),
                difficulty: diff_str.to_string(),
                score: Some(rec.score as f64),
                acc: acc_percent,
                rks,
                difficulty_value: dv,
                is_fc: rec.is_full_combo,
            });
        }
    }

    // 按 RKS 降序排序
    all_records.sort_by(|a, b| {
        b.rks
            .partial_cmp(&a.rks)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // 取前27条
    let top27: Vec<RenderRecord> = all_records.iter().take(27).cloned().collect();

    let best27_avg = top27.iter().map(|r| r.rks).sum::<f64>() / 27.0;
    let phase4_elapsed = phase4_start.elapsed();
    println!("  总成绩数: {}", all_records.len());
    println!("  Best27 平均 RKS: {best27_avg:.4}");
    println!("  耗时: {phase4_elapsed:?}\n");
    performance_report.push(format!(
        "阶段 4: 计算 RKS 并排序 - {phase4_elapsed:?} (总成绩 {}, Best27 平均 {best27_avg:.4})",
        all_records.len()
    ));

    println!("阶段 5: 计算推分 ACC...");
    let phase5_start = std::time::Instant::now();

    // 计算推分 ACC
    let mut push_acc_map: HashMap<String, f64> = HashMap::new();
    let engine_all: Vec<phi_backend::features::rks::engine::RksRecord> = all_records
        .iter()
        .filter_map(|r| {
            let diff = match r.difficulty.as_str() {
                "EZ" => Difficulty::EZ,
                "HD" => Difficulty::HD,
                "IN" => Difficulty::IN,
                "AT" => Difficulty::AT,
                _ => return None,
            };
            Some(phi_backend::features::rks::engine::RksRecord {
                song_id: r.song_id.clone(),
                difficulty: diff,
                score: r.score.unwrap_or(0.0) as u32,
                acc: r.acc,
                rks: r.rks,
                chart_constant: r.difficulty_value,
            })
        })
        .collect();

    for s in top27
        .iter()
        .filter(|s| s.acc < 100.0 && s.difficulty_value > 0.0)
    {
        let key = format!("{}-{}", s.song_id, s.difficulty);
        if let Some(v) = phi_backend::features::rks::engine::calculate_target_chart_push_acc(
            &key,
            s.difficulty_value,
            &engine_all,
        ) {
            push_acc_map.insert(key, v);
        }
    }

    let phase5_elapsed = phase5_start.elapsed();
    println!("  计算了 {} 首歌曲的推分 ACC", push_acc_map.len());
    println!("  耗时: {phase5_elapsed:?}\n");
    performance_report.push(format!(
        "阶段 5: 计算推分 ACC - {phase5_elapsed:?} (计算 {} 首)",
        push_acc_map.len()
    ));

    println!("阶段 6: 生成统计信息...");
    let phase6_start = std::time::Instant::now();

    // 生成统计信息
    use chrono::Utc;
    use phi_backend::features::image::PlayerStats;

    let (exact_rks, _rounded) =
        phi_backend::features::rks::engine::calculate_player_rks_details(&engine_all);
    let ap_scores: Vec<_> = all_records
        .iter()
        .filter(|r| r.acc >= 100.0)
        .take(3)
        .collect();
    let ap_top_3_avg = if ap_scores.len() == 3 {
        Some(ap_scores.iter().map(|r| r.rks).sum::<f64>() / 3.0)
    } else {
        None
    };
    let best_27_avg = if all_records.is_empty() {
        None
    } else {
        Some(
            all_records.iter().take(27).map(|r| r.rks).sum::<f64>()
                / (all_records.len().min(27) as f64),
        )
    };

    let stats = PlayerStats {
        ap_top_3_avg,
        best_27_avg,
        real_rks: Some(exact_rks),
        player_name: Some("性能测试用户".to_string()),
        update_time: Utc::now(),
        n: 27,
        ap_top_3_scores: all_records
            .iter()
            .filter(|r| r.acc >= 100.0)
            .take(3)
            .cloned()
            .collect(),
        challenge_rank: None,
        data_string: None,
        custom_footer_text: Some(config.branding.footer_text.clone()),
        is_user_generated: false,
    };

    let phase6_elapsed = phase6_start.elapsed();
    println!("  玩家 RKS: {exact_rks:.4}");
    if let Some(ap_avg) = ap_top_3_avg {
        println!("  AP Top3 平均: {ap_avg:.4}");
    }
    println!("  耗时: {phase6_elapsed:?}\n");
    let ap_info = if let Some(ap_avg) = ap_top_3_avg {
        format!("AP Top3 平均 {ap_avg:.4}")
    } else {
        "无 AP 成绩".to_string()
    };
    performance_report.push(format!(
        "阶段 6: 生成统计信息 - {phase6_elapsed:?} (玩家 RKS {exact_rks:.4}, {ap_info})"
    ));

    println!("阶段 7: 渲染 SVG...");
    let phase7_start = std::time::Instant::now();

    // 生成 SVG
    use phi_backend::features::image::Theme;
    let svg = phi_backend::features::image::generate_svg_string(
        &top27,
        &stats,
        Some(&push_acc_map),
        &Theme::default(),
        false,
        None,
    )
    .expect("生成 SVG 失败");

    let phase7_elapsed = phase7_start.elapsed();
    println!("  SVG 大小: {} bytes", svg.len());
    println!("  耗时: {phase7_elapsed:?}\n");
    performance_report.push(format!(
        "阶段 7: 渲染 SVG - {phase7_elapsed:?} (大小 {} bytes)",
        svg.len()
    ));

    println!("阶段 8: 渲染 PNG...");
    let phase8_start = std::time::Instant::now();

    // 渲染为 PNG
    let png = phi_backend::features::image::render_svg_to_png(svg, false).expect("渲染 PNG 失败");

    let phase8_elapsed = phase8_start.elapsed();
    println!("  PNG 大小: {} bytes", png.len());
    println!("  耗时: {phase8_elapsed:?}\n");
    performance_report.push(format!(
        "阶段 8: 渲染 PNG - {phase8_elapsed:?} (大小 {} bytes)",
        png.len()
    ));

    // 保存 PNG
    let png_path = output_dir.join("b27.png");
    let mut png_file = File::create(&png_path).expect("创建 PNG 文件失败");
    png_file.write_all(&png).expect("写入 PNG 失败");

    let total_time = start.elapsed();
    println!("========================================");
    println!("测试完成!");
    println!("  总耗时: {total_time:?}");
    println!("  输出文件: {}", png_path.display());
    println!("========================================\n");

    // 保存性能报告
    performance_report.push(String::new());
    performance_report.push("=".repeat(50));
    performance_report.push(format!("总耗时: {total_time:?}"));
    performance_report.push(format!("输出图片: {}", png_path.display()));

    let report_path = output_dir.join("performance.txt");
    let mut report_file = File::create(&report_path).expect("创建性能报告文件失败");
    report_file
        .write_all(performance_report.join("\n").as_bytes())
        .expect("写入性能报告失败");
    println!("性能报告已保存到: {}", report_path.display());

    // 生成火焰图（仅 Unix 系统）
    #[cfg(unix)]
    {
        println!("\n生成火焰图...");
        if let Ok(report) = _guard.report().build() {
            let flamegraph_path = output_dir.join("flamegraph.svg");
            let flamegraph_file = File::create(&flamegraph_path).expect("创建火焰图文件失败");
            report.flamegraph(flamegraph_file).expect("生成火焰图失败");
            println!("火焰图已保存到: {}", flamegraph_path.display());
        } else {
            eprintln!("生成火焰图失败");
        }
    }

    #[cfg(not(unix))]
    {
        println!("\n注意: 如需火焰图，请在 Linux/macOS 系统上运行此测试");
    }

    println!("\n性能分析完成!");
}
