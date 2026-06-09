use sqlx::{QueryBuilder, Row, Sqlite};

use crate::error::AppError;

use super::{
    DailyHttpRouteMetricRow, DailyHttpRouteMetricSliceRow, DailyHttpTotalMetricRow, StatsStorage,
};

fn push_daily_http_filters(
    qb: &mut QueryBuilder<'_, Sqlite>,
    route: Option<&str>,
    method: Option<&str>,
) {
    if let Some(route) = route {
        qb.push(" AND route = ").push_bind(route.to_string());
    }
    if let Some(method) = method {
        qb.push(" AND method = ").push_bind(method.to_string());
    }
}

impl StatsStorage {
    pub async fn query_daily_http_routes_with_offset(
        &self,
        modifier: &str,
        start_utc: &str,
        end_utc: &str,
        route: Option<&str>,
        method: Option<&str>,
        top_per_day: i64,
    ) -> Result<Vec<DailyHttpRouteMetricRow>, AppError> {
        let rows = if top_per_day > 0 {
            let mut qb = QueryBuilder::<Sqlite>::new(
                r"
                SELECT date, route, method, total, errors, client_errors, server_errors
                FROM (
                    SELECT date(ts_utc, 
                ",
            );
            qb.push_bind(modifier)
                .push(
                    r") as date,
                           route,
                           method,
                           COUNT(1) as total,
                           COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as errors,
                           COALESCE(SUM(CASE WHEN status BETWEEN 400 AND 499 THEN 1 ELSE 0 END), 0) as client_errors,
                           COALESCE(SUM(CASE WHEN status >= 500 THEN 1 ELSE 0 END), 0) as server_errors,
                           ROW_NUMBER() OVER (
                               PARTITION BY date(ts_utc, ",
                )
                .push_bind(modifier)
                .push(
                    r")
                               ORDER BY
                                   COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) DESC,
                                   COUNT(1) DESC,
                                   route ASC,
                                   method ASC
                           ) as rn
                    FROM events
                    WHERE route IS NOT NULL
                      AND status IS NOT NULL
                      AND ts_utc BETWEEN ",
                )
                .push_bind(start_utc.to_string())
                .push(" AND ")
                .push_bind(end_utc.to_string());

            push_daily_http_filters(&mut qb, route, method);

            qb.push(
                r"
                    GROUP BY date(ts_utc, ",
            )
            .push_bind(modifier)
            .push(
                r"), route, method
                ) ranked
                WHERE rn <= ",
            )
            .push_bind(top_per_day)
            .push(" ORDER BY date ASC, errors DESC, total DESC, route ASC, method ASC");
            qb.build()
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("daily http routes with offset: {e}")))?
        } else {
            let mut qb = QueryBuilder::<Sqlite>::new(
                r"
                SELECT date(ts_utc, ",
            );
            qb.push_bind(modifier)
                .push(
                    r") as date,
                       route,
                       method,
                       COUNT(1) as total,
                       COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as errors,
                       COALESCE(SUM(CASE WHEN status BETWEEN 400 AND 499 THEN 1 ELSE 0 END), 0) as client_errors,
                       COALESCE(SUM(CASE WHEN status >= 500 THEN 1 ELSE 0 END), 0) as server_errors
                FROM events
                WHERE route IS NOT NULL
                  AND status IS NOT NULL
                  AND ts_utc BETWEEN ",
                )
                .push_bind(start_utc.to_string())
                .push(" AND ")
                .push_bind(end_utc.to_string());

            push_daily_http_filters(&mut qb, route, method);

            qb.push(
                r"
                GROUP BY date(ts_utc, ",
            )
            .push_bind(modifier)
            .push(
                r"), route, method
                ORDER BY date ASC
            ",
            );
            qb.build()
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("daily http routes with offset: {e}")))?
        };

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(DailyHttpRouteMetricRow {
                date: r.get::<String, _>("date"),
                route: r.get::<String, _>("route"),
                method: r.get::<String, _>("method"),
                total: r.get::<i64, _>("total"),
                errors: r.get::<i64, _>("errors"),
                client_errors: r.get::<i64, _>("client_errors"),
                server_errors: r.get::<i64, _>("server_errors"),
            });
        }
        Ok(out)
    }

    pub async fn query_daily_http_totals_with_offset(
        &self,
        modifier: &str,
        start_utc: &str,
        end_utc: &str,
        route: Option<&str>,
        method: Option<&str>,
    ) -> Result<Vec<DailyHttpTotalMetricRow>, AppError> {
        let mut qb = QueryBuilder::<Sqlite>::new("SELECT date(ts_utc, ");
        qb.push_bind(modifier).push(
            ") as date, COUNT(1) as total, COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as errors, COALESCE(SUM(CASE WHEN status BETWEEN 400 AND 499 THEN 1 ELSE 0 END), 0) as client_errors, COALESCE(SUM(CASE WHEN status >= 500 THEN 1 ELSE 0 END), 0) as server_errors FROM events WHERE route IS NOT NULL AND status IS NOT NULL AND ts_utc BETWEEN ",
        )
        .push_bind(start_utc.to_string())
        .push(" AND ")
        .push_bind(end_utc.to_string());

        push_daily_http_filters(&mut qb, route, method);

        qb.push(" GROUP BY date(ts_utc, ")
            .push_bind(modifier)
            .push(") ORDER BY date ASC");

        let rows = qb
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("daily http totals with offset: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(DailyHttpTotalMetricRow {
                date: r.get::<String, _>("date"),
                total: r.get::<i64, _>("total"),
                errors: r.get::<i64, _>("errors"),
                client_errors: r.get::<i64, _>("client_errors"),
                server_errors: r.get::<i64, _>("server_errors"),
            });
        }
        Ok(out)
    }

    pub async fn query_daily_http_route_slice(
        &self,
        start_utc: &str,
        end_utc: &str,
        route: Option<&str>,
        method: Option<&str>,
    ) -> Result<Vec<DailyHttpRouteMetricSliceRow>, AppError> {
        let mut qb = QueryBuilder::<Sqlite>::new(
            r"
            SELECT route,
                   method,
                   COUNT(1) as total,
                   COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as errors,
                   COALESCE(SUM(CASE WHEN status BETWEEN 400 AND 499 THEN 1 ELSE 0 END), 0) as client_errors,
                   COALESCE(SUM(CASE WHEN status >= 500 THEN 1 ELSE 0 END), 0) as server_errors
            FROM events
            WHERE route IS NOT NULL
              AND status IS NOT NULL
              AND ts_utc BETWEEN 
        ",
        );
        qb.push_bind(start_utc.to_string())
            .push(" AND ")
            .push_bind(end_utc.to_string());
        push_daily_http_filters(&mut qb, route, method);
        qb.push(
            r"
            GROUP BY route, method
        ",
        );
        let rows = qb
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("daily http route slice: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(DailyHttpRouteMetricSliceRow {
                route: r.get::<String, _>("route"),
                method: r.get::<String, _>("method"),
                total: r.get::<i64, _>("total"),
                errors: r.get::<i64, _>("errors"),
                client_errors: r.get::<i64, _>("client_errors"),
                server_errors: r.get::<i64, _>("server_errors"),
            });
        }
        Ok(out)
    }

    pub async fn query_daily_http_route_slice_top(
        &self,
        start_utc: &str,
        end_utc: &str,
        route: Option<&str>,
        method: Option<&str>,
        top_per_day: i64,
    ) -> Result<Vec<DailyHttpRouteMetricSliceRow>, AppError> {
        let mut qb = QueryBuilder::<Sqlite>::new(
            r"
            SELECT route,
                   method,
                   COUNT(1) as total,
                   COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as errors,
                   COALESCE(SUM(CASE WHEN status BETWEEN 400 AND 499 THEN 1 ELSE 0 END), 0) as client_errors,
                   COALESCE(SUM(CASE WHEN status >= 500 THEN 1 ELSE 0 END), 0) as server_errors
            FROM events
            WHERE route IS NOT NULL
              AND status IS NOT NULL
              AND ts_utc BETWEEN 
        ",
        );
        qb.push_bind(start_utc.to_string())
            .push(" AND ")
            .push_bind(end_utc.to_string());
        push_daily_http_filters(&mut qb, route, method);
        qb.push(
            r"
            GROUP BY route, method
            ORDER BY errors DESC, total DESC, route ASC, method ASC
            LIMIT 
        ",
        )
        .push_bind(top_per_day);
        let rows = qb
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("daily http route slice top: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(DailyHttpRouteMetricSliceRow {
                route: r.get::<String, _>("route"),
                method: r.get::<String, _>("method"),
                total: r.get::<i64, _>("total"),
                errors: r.get::<i64, _>("errors"),
                client_errors: r.get::<i64, _>("client_errors"),
                server_errors: r.get::<i64, _>("server_errors"),
            });
        }
        Ok(out)
    }

    pub async fn query_daily_http_total_slice(
        &self,
        start_utc: &str,
        end_utc: &str,
        route: Option<&str>,
        method: Option<&str>,
    ) -> Result<(i64, i64, i64, i64), AppError> {
        let mut qb = QueryBuilder::<Sqlite>::new(
            r"
            SELECT COUNT(1) as total,
                   COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0) as errors,
                   COALESCE(SUM(CASE WHEN status BETWEEN 400 AND 499 THEN 1 ELSE 0 END), 0) as client_errors,
                   COALESCE(SUM(CASE WHEN status >= 500 THEN 1 ELSE 0 END), 0) as server_errors
            FROM events
            WHERE route IS NOT NULL
              AND status IS NOT NULL
              AND ts_utc BETWEEN 
        ",
        );
        qb.push_bind(start_utc.to_string())
            .push(" AND ")
            .push_bind(end_utc.to_string());
        push_daily_http_filters(&mut qb, route, method);
        let r = qb
            .build()
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("daily http total slice: {e}")))?;

        Ok((
            r.get::<i64, _>("total"),
            r.get::<i64, _>("errors"),
            r.get::<i64, _>("client_errors"),
            r.get::<i64, _>("server_errors"),
        ))
    }
}
