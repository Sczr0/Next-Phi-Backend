use std::path::Path;

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use sqlx::{sqlite::SqliteConnectOptions, ConnectOptions, Row, Sqlite, SqlitePool};

use crate::error::AppError;

use super::models::{DailyAggRow, EventInsert};

#[derive(Clone)]
pub struct StatsStorage {
    pub pool: SqlitePool,
}

impl StatsStorage {
    pub async fn connect_sqlite(path: &str, wal: bool) -> Result<Self, AppError> {
        let opt = SqliteConnectOptions::new()
            .filename(Path::new(path))
            .create_if_missing(true)
            .log_statements(tracing::log::LevelFilter::Off);
        let pool = SqlitePool::connect_with(opt).await.map_err(|e| AppError::Internal(format!("sqlite connect: {}", e)))?;
        if wal {
            sqlx::query("PRAGMA journal_mode=WAL;").execute(&pool).await.ok();
        }
        sqlx::query("PRAGMA synchronous=NORMAL;").execute(&pool).await.ok();
        sqlx::query("PRAGMA foreign_keys=ON;").execute(&pool).await.ok();
        Ok(Self { pool })
    }

    pub async fn init_schema(&self) -> Result<(), AppError> {
        let ddl = r#"
        CREATE TABLE IF NOT EXISTS events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ts_utc TEXT NOT NULL,
            route TEXT,
            feature TEXT,
            action TEXT,
            method TEXT,
            status INTEGER,
            duration_ms INTEGER,
            user_hash TEXT,
            client_ip_hash TEXT,
            instance TEXT,
            extra_json TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_events_ts ON events(ts_utc);
        CREATE INDEX IF NOT EXISTS idx_events_feature_ts ON events(feature, ts_utc);
        CREATE INDEX IF NOT EXISTS idx_events_route_ts ON events(route, ts_utc);

        CREATE TABLE IF NOT EXISTS daily_agg (
            date TEXT NOT NULL,
            feature TEXT,
            route TEXT,
            method TEXT,
            count INTEGER NOT NULL,
            err_count INTEGER NOT NULL,
            PRIMARY KEY(date, feature, route, method)
        );
        "#;
        sqlx::query(ddl).execute(&self.pool).await.map_err(|e| AppError::Internal(format!("init schema: {}", e)))?;
        Ok(())
    }

    pub async fn insert_events(&self, events: &[EventInsert]) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await.map_err(|e| AppError::Internal(format!("begin tx: {}", e)))?;
        for e in events {
            let extra = e.extra_json.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default());
            sqlx::query("INSERT INTO events(ts_utc, route, feature, action, method, status, duration_ms, user_hash, client_ip_hash, instance, extra_json) VALUES(?,?,?,?,?,?,?,?,?,?,?)")
                .bind(e.ts_utc.to_rfc3339())
                .bind(&e.route)
                .bind(&e.feature)
                .bind(&e.action)
                .bind(&e.method)
                .bind(e.status.map(|v| v as i64))
                .bind(e.duration_ms)
                .bind(&e.user_hash)
                .bind(&e.client_ip_hash)
                .bind(&e.instance)
                .bind(extra)
                .execute(&mut *tx).await.map_err(|er| AppError::Internal(format!("insert event: {}", er)))?;
        }
        tx.commit().await.map_err(|e| AppError::Internal(format!("commit: {}", e)))?;
        Ok(())
    }

    pub async fn query_daily(&self, start: NaiveDate, end: NaiveDate, feature: Option<String>) -> Result<Vec<DailyAggRow>, AppError> {
        // 若 daily_agg 尚未生成，临时从 events 动态聚合
        let start_dt = NaiveDateTime::new(start, NaiveTime::from_hms_opt(0,0,0).unwrap());
        let end_dt = NaiveDateTime::new(end, NaiveTime::from_hms_opt(23,59,59).unwrap());
        let start_s = DateTime::<Utc>::from_naive_utc_and_offset(start_dt, Utc).to_rfc3339();
        let end_s = DateTime::<Utc>::from_naive_utc_and_offset(end_dt, Utc).to_rfc3339();

        let sql = r#"
            SELECT substr(ts_utc, 1, 10) as date,
                   feature,
                   route,
                   method,
                   COUNT(1) as count,
                   SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END) as err_count
            FROM events
            WHERE ts_utc BETWEEN ? AND ?
              AND (? IS NULL OR feature = ?)
            GROUP BY date, feature, route, method
            ORDER BY date ASC
        "#;
        let rows = sqlx::query(sql)
            .bind(&start_s)
            .bind(&end_s)
            .bind(feature.as_ref())
            .bind(feature.as_ref())
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("query daily: {}", e)))?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(DailyAggRow {
                date: r.get::<String, _>("date"),
                feature: r.try_get::<String, _>("feature").ok(),
                route: r.try_get::<String, _>("route").ok(),
                method: r.try_get::<String, _>("method").ok(),
                count: r.get::<i64, _>("count"),
                err_count: r.get::<i64, _>("err_count"),
            });
        }
        Ok(out)
    }
}
