// 统计归档“对账 + 补档”工具（默认 dry-run）
//
// 用法示例：
// 1) 仅对账（不写文件）：
//    cargo run --example stats_archive_reconcile --
//
// 2) 指定生产快照目录对账：
//    cargo run --example stats_archive_reconcile -- \
//      --db "D:\\git\\2 - Phi-Backend\\phi-backend\\2026-02-13\\usage_stats.db" \
//      --archive-dir "D:\\git\\2 - Phi-Backend\\phi-backend\\2026-02-13\\stats\\v1\\events"
//
// 3) 执行补档（会写入 Parquet）：
//    cargo run --example stats_archive_reconcile -- --apply

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use chrono::{Datelike, NaiveDate};
use phi_backend::config::{AppConfig, StatsArchiveConfig};
use phi_backend::error::AppError;
use phi_backend::features::stats::archive::archive_one_day;
use phi_backend::features::stats::storage::StatsStorage;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Row, SqlitePool};

#[derive(Debug, Clone)]
struct CliArgs {
    db_path: Option<PathBuf>,
    archive_dir: Option<PathBuf>,
    from_day: Option<NaiveDate>,
    to_day: Option<NaiveDate>,
    apply: bool,
    max_days: Option<usize>,
}

impl CliArgs {
    fn parse() -> Result<Self, String> {
        let mut args = std::env::args().skip(1);
        let mut out = Self {
            db_path: None,
            archive_dir: None,
            from_day: None,
            to_day: None,
            apply: false,
            max_days: None,
        };

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-h" | "--help" => {
                    print_help();
                    std::process::exit(0);
                }
                "--db" => {
                    let v = args.next().ok_or_else(|| "--db 缺少参数".to_string())?;
                    out.db_path = Some(PathBuf::from(v));
                }
                "--archive-dir" => {
                    let v = args
                        .next()
                        .ok_or_else(|| "--archive-dir 缺少参数".to_string())?;
                    out.archive_dir = Some(PathBuf::from(v));
                }
                "--from" => {
                    let v = args.next().ok_or_else(|| "--from 缺少参数".to_string())?;
                    out.from_day = Some(parse_day(&v)?);
                }
                "--to" => {
                    let v = args.next().ok_or_else(|| "--to 缺少参数".to_string())?;
                    out.to_day = Some(parse_day(&v)?);
                }
                "--max-days" => {
                    let v = args
                        .next()
                        .ok_or_else(|| "--max-days 缺少参数".to_string())?;
                    let n = v
                        .parse::<usize>()
                        .map_err(|_| format!("--max-days 参数非法: {v}"))?;
                    if n == 0 {
                        return Err("--max-days 必须大于 0".to_string());
                    }
                    out.max_days = Some(n);
                }
                "--apply" => {
                    out.apply = true;
                }
                other => {
                    return Err(format!("未知参数: {other}"));
                }
            }
        }

        if let (Some(from), Some(to)) = (out.from_day, out.to_day)
            && from > to
        {
            return Err("--from 不能晚于 --to".to_string());
        }

        Ok(out)
    }
}

fn print_help() {
    println!("统计归档对账/补档工具");
    println!();
    println!("参数：");
    println!("  --db <path>           SQLite 路径（默认读取 config.toml 中 stats.sqlite_path）");
    println!("  --archive-dir <path>  归档目录（默认读取 config.toml 中 stats.archive.dir）");
    println!("  --from <YYYY-MM-DD>   仅处理起始日期（含）");
    println!("  --to <YYYY-MM-DD>     仅处理结束日期（含）");
    println!("  --max-days <N>        最多处理 N 天（按日期升序）");
    println!("  --apply               执行补档（默认仅对账 dry-run）");
    println!("  -h, --help            显示帮助");
}

fn parse_day(s: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| format!("日期格式错误: {s}"))
}

fn in_day_range(day: NaiveDate, from: Option<NaiveDate>, to: Option<NaiveDate>) -> bool {
    if let Some(f) = from
        && day < f
    {
        return false;
    }
    if let Some(t) = to
        && day > t
    {
        return false;
    }
    true
}

fn extract_partition_day(path: &Path) -> Option<NaiveDate> {
    let mut year: Option<i32> = None;
    let mut month: Option<u32> = None;
    let mut day: Option<u32> = None;

    for comp in path.components() {
        let seg = comp.as_os_str().to_string_lossy();
        if let Some(v) = seg.strip_prefix("year=") {
            year = v.parse::<i32>().ok();
            continue;
        }
        if let Some(v) = seg.strip_prefix("month=") {
            month = v.parse::<u32>().ok();
            continue;
        }
        if let Some(v) = seg.strip_prefix("day=") {
            day = v.parse::<u32>().ok();
            continue;
        }
    }

    NaiveDate::from_ymd_opt(year?, month?, day?)
}

fn collect_archived_days(base: &Path) -> Result<BTreeSet<NaiveDate>, AppError> {
    let mut out = BTreeSet::new();
    if !base.exists() {
        return Ok(out);
    }

    let mut stack = vec![base.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir)
            .map_err(|e| AppError::Internal(format!("read_dir {}: {e}", dir.display())))?;

        for entry in entries {
            let entry = entry.map_err(|e| {
                AppError::Internal(format!("read_dir entry {}: {e}", dir.display()))
            })?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let is_parquet = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("parquet"))
                .unwrap_or(false);
            if !is_parquet {
                continue;
            }
            if let Some(d) = extract_partition_day(&path) {
                out.insert(d);
            }
        }
    }

    Ok(out)
}

async fn load_db_days(pool: &SqlitePool) -> Result<BTreeMap<NaiveDate, i64>, AppError> {
    let rows = sqlx::query(
        "SELECT substr(ts_utc,1,10) as day, COUNT(1) as c \
         FROM events GROUP BY day ORDER BY day ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(format!("query daily counts: {e}")))?;

    let mut out = BTreeMap::new();
    for r in rows {
        let day_s: String = r
            .try_get("day")
            .map_err(|e| AppError::Internal(format!("read day: {e}")))?;
        let c: i64 = r
            .try_get("c")
            .map_err(|e| AppError::Internal(format!("read count: {e}")))?;
        let day = NaiveDate::parse_from_str(&day_s, "%Y-%m-%d")
            .map_err(|e| AppError::Internal(format!("parse day {day_s}: {e}")))?;
        out.insert(day, c);
    }
    Ok(out)
}

fn day_partition_dir(base: &Path, day: NaiveDate) -> PathBuf {
    base.join(format!("year={}", day.year()))
        .join(format!("month={:02}", day.month()))
        .join(format!("day={:02}", day.day()))
}

fn count_parquet_files_in_dir(dir: &Path) -> Result<usize, AppError> {
    if !dir.exists() {
        return Ok(0);
    }
    let mut n = 0usize;
    let entries = std::fs::read_dir(dir)
        .map_err(|e| AppError::Internal(format!("read_dir {}: {e}", dir.display())))?;
    for entry in entries {
        let entry = entry
            .map_err(|e| AppError::Internal(format!("read_dir entry {}: {e}", dir.display())))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let is_parquet = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("parquet"))
            .unwrap_or(false);
        if is_parquet {
            n += 1;
        }
    }
    Ok(n)
}

async fn connect_readonly_storage(db_path: &Path) -> Result<StatsStorage, AppError> {
    let opt = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(false)
        .read_only(true)
        .log_statements(tracing::log::LevelFilter::Off);

    let pool = SqlitePool::connect_with(opt)
        .await
        .map_err(|e| AppError::Internal(format!("sqlite connect readonly: {e}")))?;

    Ok(StatsStorage { pool })
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("ERROR: {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), AppError> {
    let args = CliArgs::parse().map_err(AppError::Validation)?;
    let cfg = AppConfig::load().map_err(|e| AppError::Internal(format!("load config: {e}")))?;

    let db_path = args
        .db_path
        .clone()
        .unwrap_or_else(|| PathBuf::from(cfg.stats.sqlite_path.clone()));
    let archive_dir = args
        .archive_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from(cfg.stats.archive.dir.clone()));

    let storage = connect_readonly_storage(&db_path).await?;
    let db_days = load_db_days(&storage.pool).await?;
    let archived_days = collect_archived_days(&archive_dir)?;

    let mut missing: Vec<(NaiveDate, i64)> = db_days
        .iter()
        .filter_map(|(day, cnt)| {
            if !in_day_range(*day, args.from_day, args.to_day) {
                return None;
            }
            if archived_days.contains(day) {
                return None;
            }
            Some((*day, *cnt))
        })
        .collect();

    if let Some(limit) = args.max_days
        && missing.len() > limit
    {
        missing.truncate(limit);
    }

    let all_rows: i64 = db_days.values().sum();
    let covered_rows: i64 = db_days
        .iter()
        .filter_map(|(d, c)| archived_days.contains(d).then_some(*c))
        .sum();
    let missing_rows: i64 = missing.iter().map(|(_, c)| *c).sum();
    let coverage = if all_rows > 0 {
        (covered_rows as f64) * 100.0 / (all_rows as f64)
    } else {
        0.0
    };

    println!("=== 统计归档对账 ===");
    println!("db: {}", db_path.display());
    println!("archive_dir: {}", archive_dir.display());
    println!("db_days: {}", db_days.len());
    println!("archived_days: {}", archived_days.len());
    println!("all_rows: {}", all_rows);
    println!("covered_rows: {}", covered_rows);
    println!("coverage: {:.2}%", coverage);
    println!("missing_days(in range): {}", missing.len());
    println!("missing_rows(in range): {}", missing_rows);
    if let Some(d) = args.from_day {
        println!("from: {d}");
    }
    if let Some(d) = args.to_day {
        println!("to: {d}");
    }
    if let Some(n) = args.max_days {
        println!("max_days: {n}");
    }

    if missing.is_empty() {
        println!("没有缺失分区，无需补档。");
        return Ok(());
    }

    println!("missing sample (最多20条):");
    for (d, c) in missing.iter().take(20) {
        println!("  - {d} ({c} rows)");
    }

    if !args.apply {
        println!("当前为 dry-run。传入 --apply 后将执行补档。");
        return Ok(());
    }

    let mut arcfg = StatsArchiveConfig::default();
    arcfg.parquet = true;
    arcfg.dir = archive_dir.to_string_lossy().to_string();
    arcfg.compress = cfg.stats.archive.compress.clone();

    println!("=== 开始补档 ===");
    let mut ok_days = 0usize;
    let mut fail_days: Vec<(NaiveDate, String)> = Vec::new();

    for (day, expected_rows) in missing {
        let part_dir = day_partition_dir(&archive_dir, day);
        let before = count_parquet_files_in_dir(&part_dir)?;
        match archive_one_day(&storage, &arcfg, day).await {
            Ok(_) => {
                let after = count_parquet_files_in_dir(&part_dir)?;
                if after <= before {
                    fail_days.push((day, "归档执行成功但未生成新的 parquet 文件".to_string()));
                    eprintln!(
                        "补档异常: {} (expected_rows={}, before_files={}, after_files={})",
                        day, expected_rows, before, after
                    );
                } else {
                    ok_days += 1;
                    println!(
                        "补档成功: {} (expected_rows={}, new_files={})",
                        day,
                        expected_rows,
                        after - before
                    );
                }
            }
            Err(e) => {
                fail_days.push((day, e.to_string()));
                eprintln!("补档失败: {} => {}", day, e);
            }
        }
    }

    println!("=== 补档完成 ===");
    println!("success_days: {ok_days}");
    println!("failed_days: {}", fail_days.len());
    if !fail_days.is_empty() {
        println!("failed detail:");
        for (d, err) in fail_days {
            println!("  - {} => {}", d, err);
        }
        return Err(AppError::Internal("存在补档失败，请修复后重试".into()));
    }

    Ok(())
}
