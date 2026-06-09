use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use sqlx::{QueryBuilder, Row, Sqlite};

use crate::error::AppError;

use super::super::models::DailyAggRow;
use super::{
    DailyAggSliceRow, DailyDauDateRow, DailyFeatureUsageDateRow, DailyFeatureUsageSliceRow,
    StatsStorage,
};

fn push_daily_agg_filters(
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

fn push_daily_feature_filter(qb: &mut QueryBuilder<'_, Sqlite>, feature: Option<&str>) {
    if let Some(feature) = feature {
        qb.push(" AND feature = ").push_bind(feature.to_string());
    }
}

const DAILY_DAU_USERS_WITH_OFFSET_SQL: &str = r"
    SELECT date(ts_utc, ?) as date,
           COUNT(DISTINCT user_hash) as active_users
    FROM events
    WHERE ts_utc BETWEEN ? AND ?
    GROUP BY date
    ORDER BY date ASC
";

const DAILY_DAU_IPS_WITH_OFFSET_SQL: &str = r"
    SELECT date(ts_utc, ?) as date,
           COUNT(DISTINCT client_ip_hash) as active_ips
    FROM events
    WHERE ts_utc BETWEEN ? AND ?
    GROUP BY date
    ORDER BY date ASC
";

const DAILY_DAU_USERS_SLICE_SQL: &str = r"
    SELECT COUNT(DISTINCT user_hash) as active_users
    FROM events
    WHERE ts_utc BETWEEN ? AND ?
";

const DAILY_DAU_IPS_SLICE_SQL: &str = r"
    SELECT COUNT(DISTINCT client_ip_hash) as active_ips
    FROM events
    WHERE ts_utc BETWEEN ? AND ?
";

fn merge_daily_dau_counts(
    user_rows: Vec<(String, i64)>,
    ip_rows: Vec<(String, i64)>,
) -> Vec<DailyDauDateRow> {
    let mut by_date: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    for (date, active_users) in user_rows {
        by_date.entry(date).or_insert((0, 0)).0 = active_users;
    }
    for (date, active_ips) in ip_rows {
        by_date.entry(date).or_insert((0, 0)).1 = active_ips;
    }

    by_date
        .into_iter()
        .map(|(date, (active_users, active_ips))| DailyDauDateRow {
            date,
            active_users,
            active_ips,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daily_dau_queries_split_user_and_ip_distinct_counts() {
        assert!(DAILY_DAU_USERS_WITH_OFFSET_SQL.contains("COUNT(DISTINCT user_hash)"));
        assert!(!DAILY_DAU_USERS_WITH_OFFSET_SQL.contains("client_ip_hash"));
        assert!(DAILY_DAU_USERS_WITH_OFFSET_SQL.contains("WHERE ts_utc BETWEEN ? AND ?"));
        assert!(DAILY_DAU_USERS_WITH_OFFSET_SQL.contains("GROUP BY date"));

        assert!(DAILY_DAU_IPS_WITH_OFFSET_SQL.contains("COUNT(DISTINCT client_ip_hash)"));
        assert!(!DAILY_DAU_IPS_WITH_OFFSET_SQL.contains("user_hash"));
        assert!(DAILY_DAU_IPS_WITH_OFFSET_SQL.contains("WHERE ts_utc BETWEEN ? AND ?"));
        assert!(DAILY_DAU_IPS_WITH_OFFSET_SQL.contains("GROUP BY date"));

        assert!(DAILY_DAU_USERS_SLICE_SQL.contains("COUNT(DISTINCT user_hash)"));
        assert!(DAILY_DAU_IPS_SLICE_SQL.contains("COUNT(DISTINCT client_ip_hash)"));
    }

    #[test]
    fn merge_daily_dau_counts_preserves_sorted_zero_sided_dates() {
        let merged = merge_daily_dau_counts(
            vec![("2026-01-02".to_string(), 3), ("2026-01-01".to_string(), 0)],
            vec![("2026-01-03".to_string(), 4), ("2026-01-02".to_string(), 1)],
        );

        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].date, "2026-01-01");
        assert_eq!(merged[0].active_users, 0);
        assert_eq!(merged[0].active_ips, 0);
        assert_eq!(merged[1].date, "2026-01-02");
        assert_eq!(merged[1].active_users, 3);
        assert_eq!(merged[1].active_ips, 1);
        assert_eq!(merged[2].date, "2026-01-03");
        assert_eq!(merged[2].active_users, 0);
        assert_eq!(merged[2].active_ips, 4);
    }
}

impl StatsStorage {
    pub async fn query_daily_agg_with_offset(
        &self,
        modifier: &str,
        start_utc: &str,
        end_utc: &str,
        feature: Option<&str>,
        route: Option<&str>,
        method: Option<&str>,
    ) -> Result<Vec<DailyAggRow>, AppError> {
        let mut qb = QueryBuilder::<Sqlite>::new(
            r"
            SELECT date(ts_utc, 
        ",
        );
        qb.push_bind(modifier.to_string())
            .push(
                r") as date,
                   feature,
                   route,
                   method,
                   COUNT(1) as count,
                   COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as err_count
            FROM events
            WHERE ts_utc BETWEEN 
        ",
            )
            .push_bind(start_utc.to_string())
            .push(" AND ")
            .push_bind(end_utc.to_string());

        push_daily_agg_filters(&mut qb, feature, route, method);

        qb.push(
            r"
            GROUP BY date, feature, route, method
            ORDER BY date ASC
        ",
        );

        let rows = qb
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("query daily with offset: {e}")))?;

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

    pub async fn query_daily_agg_slice(
        &self,
        start_utc: &str,
        end_utc: &str,
        feature: Option<&str>,
        route: Option<&str>,
        method: Option<&str>,
    ) -> Result<Vec<DailyAggSliceRow>, AppError> {
        let mut qb = QueryBuilder::<Sqlite>::new(
            r"
            SELECT feature,
                   route,
                   method,
                   COUNT(1) as count,
                   COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as err_count
            FROM events
            WHERE ts_utc BETWEEN 
        ",
        );
        qb.push_bind(start_utc.to_string())
            .push(" AND ")
            .push_bind(end_utc.to_string());

        push_daily_agg_filters(&mut qb, feature, route, method);

        qb.push(
            r"
            GROUP BY feature, route, method
        ",
        );

        let rows = qb
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("query daily slice: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(DailyAggSliceRow {
                feature: r.try_get::<String, _>("feature").ok(),
                route: r.try_get::<String, _>("route").ok(),
                method: r.try_get::<String, _>("method").ok(),
                count: r.get::<i64, _>("count"),
                err_count: r.get::<i64, _>("err_count"),
            });
        }
        Ok(out)
    }

    pub async fn query_daily_feature_usage_with_offset(
        &self,
        modifier: &str,
        start_utc: &str,
        end_utc: &str,
        feature: Option<&str>,
    ) -> Result<Vec<DailyFeatureUsageDateRow>, AppError> {
        let mut qb = QueryBuilder::<Sqlite>::new(
            r"
            SELECT date(ts_utc, 
        ",
        );
        qb.push_bind(modifier.to_string())
            .push(
                r") as date,
                   feature,
                   COUNT(1) as count,
                   COUNT(DISTINCT user_hash) as unique_users
            FROM events
            WHERE feature IS NOT NULL
              AND ts_utc BETWEEN 
        ",
            )
            .push_bind(start_utc.to_string())
            .push(" AND ")
            .push_bind(end_utc.to_string());

        push_daily_feature_filter(&mut qb, feature);

        qb.push(
            r"
            GROUP BY date, feature
            ORDER BY date ASC
        ",
        );

        let rows = qb
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("daily features with offset: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(DailyFeatureUsageDateRow {
                date: r.get::<String, _>("date"),
                feature: r.get::<String, _>("feature"),
                count: r.get::<i64, _>("count"),
                unique_users: r.get::<i64, _>("unique_users"),
            });
        }
        Ok(out)
    }

    pub async fn query_daily_feature_usage_slice(
        &self,
        start_utc: &str,
        end_utc: &str,
        feature: Option<&str>,
    ) -> Result<Vec<DailyFeatureUsageSliceRow>, AppError> {
        let mut qb = QueryBuilder::<Sqlite>::new(
            r"
            SELECT feature,
                   COUNT(1) as count,
                   COUNT(DISTINCT user_hash) as unique_users
            FROM events
            WHERE feature IS NOT NULL
              AND ts_utc BETWEEN 
        ",
        );
        qb.push_bind(start_utc.to_string())
            .push(" AND ")
            .push_bind(end_utc.to_string());

        push_daily_feature_filter(&mut qb, feature);

        qb.push(
            r"
            GROUP BY feature
        ",
        );

        let rows = qb
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("daily features slice: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(DailyFeatureUsageSliceRow {
                feature: r.get::<String, _>("feature"),
                count: r.get::<i64, _>("count"),
                unique_users: r.get::<i64, _>("unique_users"),
            });
        }
        Ok(out)
    }

    pub async fn query_daily_dau_with_offset(
        &self,
        modifier: &str,
        start_utc: &str,
        end_utc: &str,
    ) -> Result<Vec<DailyDauDateRow>, AppError> {
        let users_fut = async {
            let rows = sqlx::query(DAILY_DAU_USERS_WITH_OFFSET_SQL)
                .bind(modifier)
                .bind(start_utc)
                .bind(end_utc)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("daily dau users with offset: {e}")))?;

            Ok::<Vec<(String, i64)>, AppError>(
                rows.into_iter()
                    .map(|r| (r.get::<String, _>("date"), r.get::<i64, _>("active_users")))
                    .collect(),
            )
        };
        let ips_fut = async {
            let rows = sqlx::query(DAILY_DAU_IPS_WITH_OFFSET_SQL)
                .bind(modifier)
                .bind(start_utc)
                .bind(end_utc)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("daily dau ips with offset: {e}")))?;

            Ok::<Vec<(String, i64)>, AppError>(
                rows.into_iter()
                    .map(|r| (r.get::<String, _>("date"), r.get::<i64, _>("active_ips")))
                    .collect(),
            )
        };

        let (user_rows, ip_rows) = tokio::try_join!(users_fut, ips_fut)?;
        Ok(merge_daily_dau_counts(user_rows, ip_rows))
    }

    pub async fn query_daily_dau_slice(
        &self,
        start_utc: &str,
        end_utc: &str,
    ) -> Result<(i64, i64), AppError> {
        let users_fut = async {
            let r = sqlx::query(DAILY_DAU_USERS_SLICE_SQL)
                .bind(start_utc)
                .bind(end_utc)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("daily dau users slice: {e}")))?;
            Ok::<i64, AppError>(r.get::<i64, _>("active_users"))
        };
        let ips_fut = async {
            let r = sqlx::query(DAILY_DAU_IPS_SLICE_SQL)
                .bind(start_utc)
                .bind(end_utc)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("daily dau ips slice: {e}")))?;
            Ok::<i64, AppError>(r.get::<i64, _>("active_ips"))
        };

        tokio::try_join!(users_fut, ips_fut)
    }

    pub async fn query_daily(
        &self,
        start: NaiveDate,
        end: NaiveDate,
        feature: Option<String>,
        route: Option<String>,
        method: Option<String>,
    ) -> Result<Vec<DailyAggRow>, AppError> {
        // 若 daily_agg 尚未生成，临时从 events 动态聚合
        let start_dt = NaiveDateTime::new(start, NaiveTime::from_hms_opt(0, 0, 0).unwrap());
        let end_dt = NaiveDateTime::new(end, NaiveTime::from_hms_opt(23, 59, 59).unwrap());
        let start_s = DateTime::<Utc>::from_naive_utc_and_offset(start_dt, Utc).to_rfc3339();
        let end_s = DateTime::<Utc>::from_naive_utc_and_offset(end_dt, Utc).to_rfc3339();

        let mut qb = QueryBuilder::<Sqlite>::new(
            r"
            SELECT substr(ts_utc, 1, 10) as date,
                   feature,
                   route,
                   method,
                   COUNT(1) as count,
                   SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END) as err_count
            FROM events
            WHERE ts_utc BETWEEN 
        ",
        );
        qb.push_bind(start_s).push(" AND ").push_bind(end_s);

        push_daily_agg_filters(
            &mut qb,
            feature.as_deref(),
            route.as_deref(),
            method.as_deref(),
        );

        qb.push(
            r"
            GROUP BY date, feature, route, method
            ORDER BY date ASC
        ",
        );

        let rows = qb
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("query daily: {e}")))?;
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
