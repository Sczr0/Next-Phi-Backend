use sqlx::{QueryBuilder, Row, Sqlite};

use crate::error::AppError;

use super::{LatencyAggBucketRow, LatencyAggSliceRow, StatsStorage};

fn push_latency_filters(
    qb: &mut QueryBuilder<'_, Sqlite>,
    feature: Option<&str>,
    route: Option<&str>,
    method: Option<&str>,
) {
    if let Some(feature) = feature {
        qb.push(" AND feature = ").push_bind(feature.to_string());
    }
    if let Some(route) = route {
        qb.push(" AND route = ").push_bind(route.to_string());
    }
    if let Some(method) = method {
        qb.push(" AND method = ").push_bind(method.to_string());
    }
}

impl StatsStorage {
    pub async fn query_latency_agg_with_offset(
        &self,
        modifier: &str,
        start_utc: &str,
        end_utc: &str,
        feature: Option<&str>,
        route: Option<&str>,
        method: Option<&str>,
    ) -> Result<Vec<LatencyAggBucketRow>, AppError> {
        let mut qb = QueryBuilder::<Sqlite>::new(
            r"
            SELECT date(ts_utc, 
        ",
        );
        qb.push_bind(modifier.to_string())
            .push(
                r") as bucket,
                   feature,
                   route,
                   method,
                   COUNT(1) as count,
                   MIN(duration_ms) as min_ms,
                   AVG(duration_ms) as avg_ms,
                   MAX(duration_ms) as max_ms
            FROM events
            WHERE route IS NOT NULL
              AND duration_ms IS NOT NULL
              AND ts_utc BETWEEN 
        ",
            )
            .push_bind(start_utc.to_string())
            .push(" AND ")
            .push_bind(end_utc.to_string());
        push_latency_filters(&mut qb, feature, route, method);
        qb.push(
            r"
            GROUP BY bucket, feature, route, method
            ORDER BY bucket ASC, route ASC, method ASC
        ",
        );
        let rows = qb
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("latency agg with offset: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(LatencyAggBucketRow {
                bucket: r.get::<String, _>("bucket"),
                feature: r.try_get::<String, _>("feature").ok(),
                route: r.try_get::<String, _>("route").ok(),
                method: r.try_get::<String, _>("method").ok(),
                count: r.get::<i64, _>("count"),
                min_ms: r.try_get::<i64, _>("min_ms").ok(),
                avg_ms: r.try_get::<f64, _>("avg_ms").ok(),
                max_ms: r.try_get::<i64, _>("max_ms").ok(),
            });
        }
        Ok(out)
    }

    pub async fn query_latency_agg_slice(
        &self,
        start_utc: &str,
        end_utc: &str,
        feature: Option<&str>,
        route: Option<&str>,
        method: Option<&str>,
    ) -> Result<Vec<LatencyAggSliceRow>, AppError> {
        let mut qb = QueryBuilder::<Sqlite>::new(
            r"
            SELECT feature,
                   route,
                   method,
                   COUNT(1) as count,
                   MIN(duration_ms) as min_ms,
                   AVG(duration_ms) as avg_ms,
                   MAX(duration_ms) as max_ms
            FROM events
            WHERE route IS NOT NULL
              AND duration_ms IS NOT NULL
              AND ts_utc BETWEEN 
        ",
        );
        qb.push_bind(start_utc.to_string())
            .push(" AND ")
            .push_bind(end_utc.to_string());
        push_latency_filters(&mut qb, feature, route, method);
        qb.push(
            r"
            GROUP BY feature, route, method
            ORDER BY route ASC, method ASC
        ",
        );
        let rows = qb
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("latency agg slice: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(LatencyAggSliceRow {
                feature: r.try_get::<String, _>("feature").ok(),
                route: r.try_get::<String, _>("route").ok(),
                method: r.try_get::<String, _>("method").ok(),
                count: r.get::<i64, _>("count"),
                min_ms: r.try_get::<i64, _>("min_ms").ok(),
                avg_ms: r.try_get::<f64, _>("avg_ms").ok(),
                max_ms: r.try_get::<i64, _>("max_ms").ok(),
            });
        }
        Ok(out)
    }

    // ── 延迟百分位近似查询（直方图，避免全量排序） ──

    /// 用等宽直方图近似延迟分布的 p50/p95，避免 ROW_NUMBER() OVER (ORDER BY) 全量排序。
    /// 返回 (avg_ms, p50_ms, p95_ms, max_ms) 和采样数
    pub async fn query_latency_percentiles_histogram(
        &self,
        start_utc: &str,
        end_utc: &str,
    ) -> Result<super::SummaryLatencyData, AppError> {
        // 1. 先取基本统计量（AVG, MAX）
        let stats_row = sqlx::query(
            r"
            SELECT
                COUNT(1) AS n,
                AVG(duration_ms) AS avg,
                MAX(duration_ms) AS max
            FROM events
            WHERE route IS NOT NULL
              AND duration_ms IS NOT NULL
              AND ts_utc BETWEEN ? AND ?
            ",
        )
        .bind(start_utc)
        .bind(end_utc)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("latency histogram stats: {e}")))?;

        let n: i64 = stats_row.try_get("n").unwrap_or(0);
        let avg_ms: Option<f64> = stats_row.try_get("avg").ok();
        let max_ms: Option<i64> = stats_row.try_get("max").ok();

        if n < 2 || max_ms.unwrap_or(0) <= 0 {
            return Ok(super::SummaryLatencyData {
                sample_count: n,
                avg_ms,
                p50_ms: None,
                p95_ms: None,
                max_ms,
            });
        }

        let max = max_ms.unwrap_or(0).max(1);

        // 2. 分桶直方图（自适应桶数：最多 50 桶，每桶至少 1ms）
        let bucket_count = 50i64;
        let bucket_width = (max / bucket_count).max(1);

        let buckets = sqlx::query(
            r"
            SELECT
                CAST(duration_ms / ? AS INTEGER) * ? AS bucket_lower,
                COUNT(1) AS cnt
            FROM events
            WHERE route IS NOT NULL
              AND duration_ms IS NOT NULL
              AND ts_utc BETWEEN ? AND ?
            GROUP BY bucket_lower
            ORDER BY bucket_lower
            ",
        )
        .bind(bucket_width)
        .bind(bucket_width)
        .bind(start_utc)
        .bind(end_utc)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("latency histogram buckets: {e}")))?;

        struct HistogramBucket {
            bucket_lower: i64,
            cnt: i64,
        }
        fn row_to_bucket(r: &sqlx::sqlite::SqliteRow) -> HistogramBucket {
            HistogramBucket {
                bucket_lower: r.get("bucket_lower"),
                cnt: r.get("cnt"),
            }
        }
        let rows: Vec<HistogramBucket> = buckets.iter().map(row_to_bucket).collect();

        // 3. 从直方图推算 p50, p95
        #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
        let p50_target = (n as f64 * 0.50).ceil() as i64;
        #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
        let p95_target = (n as f64 * 0.95).ceil() as i64;

        let mut cumulative = 0i64;
        let mut p50_ms: Option<i64> = None;
        let mut p95_ms: Option<i64> = None;

        for bucket in &rows {
            cumulative += bucket.cnt;
            if p50_ms.is_none() && cumulative >= p50_target {
                p50_ms = Some(bucket.bucket_lower + bucket_width / 2);
            }
            if p95_ms.is_none() && cumulative >= p95_target {
                p95_ms = Some(bucket.bucket_lower + bucket_width / 2);
            }
            if p50_ms.is_some() && p95_ms.is_some() {
                break;
            }
        }

        Ok(super::SummaryLatencyData {
            sample_count: n,
            avg_ms,
            p50_ms,
            p95_ms,
            max_ms,
        })
    }
}
