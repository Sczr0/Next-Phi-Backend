use std::{
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

pub async fn run_daily_archiver(storage: Arc<StatsStorage>, cfg: StatsConfig) {
    // 初次启动时：等待到下一个每日时间点
    loop {
        let now = chrono::Local::now();
        let target = parse_today_time(&cfg.daily_aggregate_time).unwrap_or((3, 0));
        let next = next_occurrence(now, target.0, target.1);
        let sleep_dur = (next - now).to_std().unwrap_or(Duration::from_secs(60));
        tracing::info!("统计归档：将在 {} 触发", next);
        tokio::time::sleep(sleep_dur).await;

        // 归档前一日
        let yday = (now - chrono::Duration::days(1)).date_naive();
        if let Err(e) = archive_one_day(&storage, &cfg.archive, yday).await {
            tracing::warn!("统计归档失败: {}", e);
        }
        // 清理策略可在此扩展（按 cfg.retention_hot_days 删除旧 events）
    }
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
        let compression = match compress.to_ascii_lowercase().as_str() {
            "snappy" => Compression::SNAPPY,
            "zstd" => Compression::ZSTD(ZstdLevel::default()),
            _ => Compression::UNCOMPRESSED,
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
