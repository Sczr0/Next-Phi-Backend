use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use arrow_array::builder::{Int64Builder, StringBuilder, UInt16Builder};
use arrow_array::{ArrayRef, RecordBatch, builder::TimestampMillisecondBuilder};
use arrow_schema::{DataType, Field, Schema};
use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use parquet::arrow::ArrowWriter;
use parquet::basic::{Compression, ZstdLevel};
use parquet::file::properties::WriterProperties;
use sqlx::Row;

use crate::config::{StatsArchiveConfig, StatsConfig};
use crate::error::AppError;

use super::storage::StatsStorage;

// 启动时只做轻量补档，避免冷启动长时间占用 IO。
const STARTUP_BACKFILL_MAX_DAYS: usize = 7;
// 每日维护允许补齐的缺口上限，防止单次任务过重。
const DAILY_BACKFILL_MAX_DAYS: usize = 30;
// 历史清理按批次删除，避免长事务锁住写入。
const CLEANUP_DELETE_BATCH_SIZE: i64 = 5000;

#[derive(Default, Debug, Clone, Copy)]
struct ReconcileStats {
    missing_days: usize,
    backfilled_days: usize,
}

#[derive(Default, Debug, Clone, Copy)]
struct CleanupStats {
    candidate_days: usize,
    skipped_unarchived_days: usize,
    deleted_days: usize,
    deleted_rows: i64,
}

pub async fn run_daily_archiver(storage: Arc<StatsStorage>, cfg: StatsConfig) {
    // 启动后先执行一次轻量维护（限额补档 + 清理），避免长期缺口一直积累。
    if let Err(e) = run_maintenance_once(&storage, &cfg, Some(STARTUP_BACKFILL_MAX_DAYS)).await {
        tracing::warn!("统计维护（启动补偿）失败: {}", e);
    }

    // 常规每日维护：在固定时间点触发。
    loop {
        let now = chrono::Local::now();
        let target = parse_today_time(&cfg.daily_aggregate_time).unwrap_or((3, 0));
        let next = next_occurrence(now, target.0, target.1);
        let sleep_dur = (next - now).to_std().unwrap_or(Duration::from_secs(60));
        tracing::info!("统计维护：将在 {} 触发", next);
        tokio::time::sleep(sleep_dur).await;

        if let Err(e) = run_maintenance_once(&storage, &cfg, Some(DAILY_BACKFILL_MAX_DAYS)).await {
            tracing::warn!("统计维护失败: {}", e);
        }
    }
}

async fn run_maintenance_once(
    storage: &StatsStorage,
    cfg: &StatsConfig,
    max_backfill_days: Option<usize>,
) -> Result<(), AppError> {
    if !cfg.archive.parquet {
        return Ok(());
    }

    let reconcile = reconcile_missing_archives(storage, &cfg.archive, max_backfill_days).await?;
    let cleanup =
        cleanup_archived_hot_events(storage, &cfg.archive, cfg.retention_hot_days).await?;

    if cleanup.deleted_rows > 0
        && let Err(e) = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE);")
            .execute(&storage.pool)
            .await
    {
        tracing::warn!("统计维护：checkpoint 失败: {}", e);
    }

    tracing::info!(
        "统计维护完成: missing_days={}, backfilled_days={}, candidate_days={}, skipped_unarchived_days={}, deleted_days={}, deleted_rows={}",
        reconcile.missing_days,
        reconcile.backfilled_days,
        cleanup.candidate_days,
        cleanup.skipped_unarchived_days,
        cleanup.deleted_days,
        cleanup.deleted_rows
    );
    Ok(())
}

async fn reconcile_missing_archives(
    storage: &StatsStorage,
    arcfg: &StatsArchiveConfig,
    max_backfill_days: Option<usize>,
) -> Result<ReconcileStats, AppError> {
    if !arcfg.parquet {
        return Ok(ReconcileStats::default());
    }

    let db_day_counts = load_db_day_counts(storage).await?;
    if db_day_counts.is_empty() {
        return Ok(ReconcileStats::default());
    }
    let archived_days = collect_archived_days(Path::new(&arcfg.dir))?;
    // 仅归档到“昨日 UTC”，避免把当天正在增长的数据重复归档。
    let archive_cutoff = Utc::now().date_naive() - chrono::Duration::days(1);

    let mut missing_days: Vec<NaiveDate> = db_day_counts
        .keys()
        .filter(|d| **d <= archive_cutoff && !archived_days.contains(d))
        .copied()
        .collect();
    missing_days.sort_unstable();

    let total_missing = missing_days.len();
    if let Some(limit) = max_backfill_days
        && missing_days.len() > limit
    {
        missing_days.truncate(limit);
    }

    let mut backfilled = 0usize;
    for day in missing_days {
        archive_one_day(storage, arcfg, day).await?;
        backfilled += 1;
    }

    Ok(ReconcileStats {
        missing_days: total_missing,
        backfilled_days: backfilled,
    })
}

async fn cleanup_archived_hot_events(
    storage: &StatsStorage,
    arcfg: &StatsArchiveConfig,
    retention_hot_days: u32,
) -> Result<CleanupStats, AppError> {
    if retention_hot_days == 0 {
        return Ok(CleanupStats::default());
    }

    let db_day_counts = load_db_day_counts(storage).await?;
    if db_day_counts.is_empty() {
        return Ok(CleanupStats::default());
    }

    let latest_day = *db_day_counts
        .keys()
        .last()
        .ok_or_else(|| AppError::Internal("无法获取最新统计日期".into()))?;
    let keep_start = latest_day - chrono::Duration::days((retention_hot_days - 1) as i64);
    let archived_days = collect_archived_days(Path::new(&arcfg.dir))?;

    let mut stats = CleanupStats::default();
    for day in db_day_counts.keys().copied() {
        if day >= keep_start {
            continue;
        }
        stats.candidate_days += 1;
        if !archived_days.contains(&day) {
            stats.skipped_unarchived_days += 1;
            continue;
        }
        let deleted = delete_one_day_in_batches(storage, day, CLEANUP_DELETE_BATCH_SIZE).await?;
        if deleted > 0 {
            let remain = count_one_day_events(storage, day).await?;
            if remain != 0 {
                return Err(AppError::Internal(format!(
                    "清理后仍有残留数据: day={}, remain={}",
                    day, remain
                )));
            }
            stats.deleted_days += 1;
            stats.deleted_rows += deleted;
        }
    }

    if stats.skipped_unarchived_days > 0 {
        tracing::warn!(
            "统计维护：存在 {} 个未归档历史分区，已跳过删除",
            stats.skipped_unarchived_days
        );
    }

    Ok(stats)
}

async fn load_db_day_counts(storage: &StatsStorage) -> Result<BTreeMap<NaiveDate, i64>, AppError> {
    let rows = sqlx::query(
        "SELECT substr(ts_utc,1,10) as day, COUNT(1) as c \
         FROM events GROUP BY day ORDER BY day ASC",
    )
    .fetch_all(&storage.pool)
    .await
    .map_err(|e| AppError::Internal(format!("load daily counts: {e}")))?;

    let mut out = BTreeMap::new();
    for r in rows {
        let day_s: String = r
            .try_get("day")
            .map_err(|e| AppError::Internal(format!("read day: {e}")))?;
        let c: i64 = r
            .try_get("c")
            .map_err(|e| AppError::Internal(format!("read day count: {e}")))?;
        let day = NaiveDate::parse_from_str(&day_s, "%Y-%m-%d")
            .map_err(|e| AppError::Internal(format!("parse day {day_s}: {e}")))?;
        out.insert(day, c);
    }
    Ok(out)
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
            if let Some(day) = extract_partition_day(&path) {
                out.insert(day);
            }
        }
    }
    Ok(out)
}

fn extract_partition_day(path: &Path) -> Option<NaiveDate> {
    let mut year: Option<i32> = None;
    let mut month: Option<u32> = None;
    let mut day: Option<u32> = None;

    for comp in path.components() {
        let seg = comp.as_os_str().to_string_lossy();
        if let Some(v) = seg.strip_prefix("year=") {
            year = v.parse::<i32>().ok();
        } else if let Some(v) = seg.strip_prefix("month=") {
            month = v.parse::<u32>().ok();
        } else if let Some(v) = seg.strip_prefix("day=") {
            day = v.parse::<u32>().ok();
        }
    }
    NaiveDate::from_ymd_opt(year?, month?, day?)
}

async fn delete_one_day_in_batches(
    storage: &StatsStorage,
    day: NaiveDate,
    batch_size: i64,
) -> Result<i64, AppError> {
    let from = day.format("%Y-%m-%d").to_string();
    let to = (day + chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    let mut total_deleted = 0i64;

    loop {
        let res = sqlx::query(
            "DELETE FROM events WHERE id IN (
               SELECT id FROM events
               WHERE ts_utc >= ? AND ts_utc < ?
               ORDER BY id ASC
               LIMIT ?
             )",
        )
        .bind(&from)
        .bind(&to)
        .bind(batch_size)
        .execute(&storage.pool)
        .await
        .map_err(|e| AppError::Internal(format!("delete events day {}: {e}", day)))?;
        let affected = res.rows_affected() as i64;
        if affected == 0 {
            break;
        }
        total_deleted += affected;
    }

    Ok(total_deleted)
}

async fn count_one_day_events(storage: &StatsStorage, day: NaiveDate) -> Result<i64, AppError> {
    let from = day.format("%Y-%m-%d").to_string();
    let to = (day + chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    let row = sqlx::query("SELECT COUNT(1) as c FROM events WHERE ts_utc >= ? AND ts_utc < ?")
        .bind(from)
        .bind(to)
        .fetch_one(&storage.pool)
        .await
        .map_err(|e| AppError::Internal(format!("count events day {}: {e}", day)))?;
    row.try_get::<i64, _>("c")
        .map_err(|e| AppError::Internal(format!("read events day count {}: {e}", day)))
}

fn parse_today_time(s: &str) -> Option<(u32, u32)> {
    let parts: Vec<_> = s.split(':').collect();
    if parts.len() != 2 {
        return None;
    }
    let h = u32::from_str(parts[0]).ok()?;
    let m = u32::from_str(parts[1]).ok()?;
    Some((h.min(23), m.min(59)))
}

fn next_occurrence(
    now: chrono::DateTime<chrono::Local>,
    hh: u32,
    mm: u32,
) -> chrono::DateTime<chrono::Local> {
    let today = now.date_naive();
    let target = NaiveDateTime::new(today, NaiveTime::from_hms_opt(hh, mm, 0).unwrap());
    let candidate = chrono::Local
        .from_local_datetime(&target)
        .single()
        .unwrap_or(now);
    if candidate > now {
        candidate
    } else {
        candidate + chrono::Duration::days(1)
    }
}

pub async fn archive_one_day(
    storage: &StatsStorage,
    arcfg: &StatsArchiveConfig,
    day: NaiveDate,
) -> Result<(), AppError> {
    if !arcfg.parquet {
        return Ok(());
    }
    let start = Utc.from_utc_datetime(&day.and_hms_opt(0, 0, 0).unwrap());
    let end = Utc.from_utc_datetime(&day.and_hms_opt(23, 59, 59).unwrap());
    let rows = sqlx::query(r#"SELECT ts_utc, route, feature, action, method, status, duration_ms, user_hash, client_ip_hash, instance, extra_json FROM events WHERE ts_utc BETWEEN ? AND ? ORDER BY ts_utc ASC"#)
        .bind(start.to_rfc3339())
        .bind(end.to_rfc3339())
        .fetch_all(&storage.pool)
        .await
        .map_err(|e| AppError::Internal(format!("archive query: {e}")))?;

    if rows.is_empty() {
        tracing::info!("统计归档：{} 无数据，跳过", day);
        return Ok(());
    }

    // 构建 Arrow 批次
    let mut tsb = TimestampMillisecondBuilder::new();
    let mut route_b = StringBuilder::new();
    let mut feature_b = StringBuilder::new();
    let mut action_b = StringBuilder::new();
    let mut method_b = StringBuilder::new();
    let mut status_b = UInt16Builder::new();
    let mut dur_b = Int64Builder::new();
    let mut user_b = StringBuilder::new();
    let mut ip_b = StringBuilder::new();
    let mut inst_b = StringBuilder::new();
    let mut extra_b = StringBuilder::new();

    for r in rows {
        let ts_s: String = r.try_get("ts_utc").unwrap_or_default();
        let ts = chrono::DateTime::parse_from_rfc3339(&ts_s)
            .ok()
            .map(|dt| dt.with_timezone(&Utc));
        if let Some(t) = ts {
            tsb.append_value(t.timestamp_millis());
        } else {
            tsb.append_null();
        }

        append_opt_string(&mut route_b, r.try_get("route").ok());
        append_opt_string(&mut feature_b, r.try_get("feature").ok());
        append_opt_string(&mut action_b, r.try_get("action").ok());
        append_opt_string(&mut method_b, r.try_get("method").ok());

        match r.try_get::<i64, _>("status").ok().map(|v| v as u16) {
            Some(v) => status_b.append_value(v),
            None => status_b.append_null(),
        }
        match r.try_get::<i64, _>("duration_ms").ok() {
            Some(v) => dur_b.append_value(v),
            None => dur_b.append_null(),
        }

        append_opt_string(&mut user_b, r.try_get("user_hash").ok());
        append_opt_string(&mut ip_b, r.try_get("client_ip_hash").ok());
        append_opt_string(&mut inst_b, r.try_get("instance").ok());
        append_opt_string(&mut extra_b, r.try_get("extra_json").ok());
    }

    let schema = std::sync::Arc::new(Schema::new(vec![
        Field::new(
            "ts_utc",
            DataType::Timestamp(arrow_schema::TimeUnit::Millisecond, None),
            true,
        ),
        Field::new("route", DataType::Utf8, true),
        Field::new("feature", DataType::Utf8, true),
        Field::new("action", DataType::Utf8, true),
        Field::new("method", DataType::Utf8, true),
        Field::new("status", DataType::UInt16, true),
        Field::new("duration_ms", DataType::Int64, true),
        Field::new("user_hash", DataType::Utf8, true),
        Field::new("client_ip_hash", DataType::Utf8, true),
        Field::new("instance", DataType::Utf8, true),
        Field::new("extra_json", DataType::Utf8, true),
    ]));

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            std::sync::Arc::new(tsb.finish()) as ArrayRef,
            std::sync::Arc::new(route_b.finish()),
            std::sync::Arc::new(feature_b.finish()),
            std::sync::Arc::new(action_b.finish()),
            std::sync::Arc::new(method_b.finish()),
            std::sync::Arc::new(status_b.finish()),
            std::sync::Arc::new(dur_b.finish()),
            std::sync::Arc::new(user_b.finish()),
            std::sync::Arc::new(ip_b.finish()),
            std::sync::Arc::new(inst_b.finish()),
            std::sync::Arc::new(extra_b.finish()),
        ],
    )
    .map_err(|e| AppError::Internal(format!("build record batch: {e}")))?;

    // 输出路径
    let out_dir = partition_dir(&arcfg.dir, day);
    tokio::fs::create_dir_all(&out_dir).await.ok();
    let file = unique_file_in(&out_dir);

    // 文件创建与 Parquet 写入属于同步 IO/CPU 密集任务，避免阻塞 Tokio worker：offload 到 blocking 线程池。
    let file_path = file.clone();
    let rows = batch.num_rows();
    let compress = arcfg.compress.clone();
    let join = tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let f = std::fs::File::create(&file_path)
            .map_err(|e| AppError::Internal(format!("create parquet: {e}")))?;

        // 压缩设置
        let compression = if compress.eq_ignore_ascii_case("snappy") {
            Compression::SNAPPY
        } else if compress.eq_ignore_ascii_case("zstd") {
            Compression::ZSTD(ZstdLevel::default())
        } else {
            Compression::UNCOMPRESSED
        };
        let props = WriterProperties::builder()
            .set_compression(compression)
            .build();
        let mut writer = ArrowWriter::try_new(f, schema, Some(props))
            .map_err(|e| AppError::Internal(format!("arrow writer: {e}")))?;
        writer
            .write(&batch)
            .map_err(|e| AppError::Internal(format!("write batch: {e}")))?;
        writer
            .close()
            .map_err(|e| AppError::Internal(format!("close parquet: {e}")))?;
        Ok(())
    })
    .await;
    match join {
        Ok(r) => r?,
        Err(e) => {
            let e_str = e.to_string();
            if let Ok(panic) = e.try_into_panic() {
                std::panic::resume_unwind(panic);
            }
            return Err(AppError::Internal(format!(
                "spawn_blocking cancelled: {e_str}"
            )));
        }
    }
    tracing::info!("统计归档完成: {} (rows={})", file.display(), rows);
    Ok(())
}

fn partition_dir(base: &str, day: NaiveDate) -> PathBuf {
    let y = day.year();
    let m = day.month();
    let d = day.day();
    PathBuf::from(base)
        .join(format!("year={y}"))
        .join(format!("month={m:02}"))
        .join(format!("day={d:02}"))
}

fn unique_file_in(dir: &Path) -> PathBuf {
    let name = format!("events-{}.parquet", uuid::Uuid::new_v4());
    dir.join(name)
}

fn append_opt_string(b: &mut StringBuilder, v: Option<String>) {
    match v {
        Some(s) => b.append_value(s),
        None => b.append_null(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use chrono::{NaiveDate, TimeZone, Utc};

    use crate::config::StatsConfig;

    use super::{
        archive_one_day, cleanup_archived_hot_events, collect_archived_days, count_one_day_events,
        run_maintenance_once,
    };
    use crate::features::stats::{models::EventInsert, storage::StatsStorage};

    fn temp_paths(prefix: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "phi_stats_archive_test_{}_{}",
            prefix,
            uuid::Uuid::new_v4()
        ));
        let db_path = root.join("usage_stats.db");
        let archive_dir = root.join("stats").join("v1").join("events");
        (root, db_path, archive_dir)
    }

    fn build_event(day: NaiveDate) -> EventInsert {
        let ts_utc = Utc.from_utc_datetime(&day.and_hms_opt(12, 0, 0).unwrap());
        EventInsert {
            ts_utc,
            route: Some("/api/v1/test".to_string()),
            feature: Some("test_feature".to_string()),
            action: Some("test_action".to_string()),
            method: Some("GET".to_string()),
            status: Some(200),
            duration_ms: Some(10),
            user_hash: None,
            client_ip_hash: None,
            instance: Some("unit_test".into()),
            extra_json: None,
        }
    }

    async fn build_storage(db_path: &Path) -> StatsStorage {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).expect("create temp db dir");
        }
        let storage = StatsStorage::connect_sqlite(db_path.to_string_lossy().as_ref(), false)
            .await
            .expect("connect sqlite");
        storage.init_schema().await.expect("init schema");
        storage
    }

    #[tokio::test]
    async fn maintenance_backfills_missing_and_cleans_archived_old_days() {
        let (root, db_path, archive_dir) = temp_paths("maintenance");
        let storage = build_storage(&db_path).await;
        std::fs::create_dir_all(&archive_dir).expect("create archive dir");

        let today = Utc::now().date_naive();
        let d1 = today - chrono::Duration::days(4);
        let d2 = today - chrono::Duration::days(3);
        let d3 = today - chrono::Duration::days(2);
        storage
            .insert_events(&[build_event(d1), build_event(d2), build_event(d3)])
            .await
            .expect("seed events");

        let mut cfg = StatsConfig::default();
        cfg.archive.parquet = true;
        cfg.archive.dir = archive_dir.to_string_lossy().to_string();
        cfg.retention_hot_days = 2;

        run_maintenance_once(&storage, &cfg, Some(10))
            .await
            .expect("run maintenance");

        let archived = collect_archived_days(archive_dir.as_path()).expect("collect archived days");
        assert!(archived.contains(&d1));
        assert!(archived.contains(&d2));
        assert!(archived.contains(&d3));

        assert_eq!(
            count_one_day_events(&storage, d1).await.expect("count d1"),
            0
        );
        assert!(count_one_day_events(&storage, d2).await.expect("count d2") > 0);
        assert!(count_one_day_events(&storage, d3).await.expect("count d3") > 0);

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn cleanup_skips_unarchived_days() {
        let (root, db_path, archive_dir) = temp_paths("cleanup_guard");
        let storage = build_storage(&db_path).await;
        std::fs::create_dir_all(&archive_dir).expect("create archive dir");

        let today = Utc::now().date_naive();
        let old_day = today - chrono::Duration::days(3);
        let latest_day = today - chrono::Duration::days(2);
        storage
            .insert_events(&[build_event(old_day), build_event(latest_day)])
            .await
            .expect("seed events");

        // 仅归档最新日，不归档 old_day，后续清理必须跳过 old_day。
        let mut cfg = StatsConfig::default();
        cfg.archive.parquet = true;
        cfg.archive.dir = archive_dir.to_string_lossy().to_string();
        archive_one_day(&storage, &cfg.archive, latest_day)
            .await
            .expect("archive latest day");

        let stats = cleanup_archived_hot_events(&storage, &cfg.archive, 1)
            .await
            .expect("cleanup");
        assert_eq!(stats.deleted_rows, 0);
        assert_eq!(stats.skipped_unarchived_days, 1);
        assert!(
            count_one_day_events(&storage, old_day)
                .await
                .expect("count old day")
                > 0
        );

        let _ = std::fs::remove_dir_all(root);
    }
}
