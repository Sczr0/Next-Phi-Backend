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
}
