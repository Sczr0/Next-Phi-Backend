use sqlx::{QueryBuilder, Row, Sqlite};

use crate::error::AppError;

use super::super::models::EventInsert;
use super::{ArchiveEventRow, StatsStorage};

fn saturating_u64_to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

const LIST_EVENT_DAY_COUNTS_SQL: &str = "SELECT substr(ts_utc,1,10) as day, COUNT(1) as c
             FROM events GROUP BY substr(ts_utc,1,10) ORDER BY day ASC";
const DELETE_EVENTS_IN_RANGE_BATCH_SQL: &str = "DELETE FROM events WHERE id IN (
               SELECT id FROM events
               WHERE ts_utc >= ? AND ts_utc < ?
               ORDER BY ts_utc ASC, id ASC
               LIMIT ?
             )";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_day_count_query_uses_day_expression_for_grouping() {
        let alias_grouping = ["GROUP BY ", "day"].concat();
        assert!(LIST_EVENT_DAY_COUNTS_SQL.contains("substr(ts_utc,1,10)"));
        assert!(LIST_EVENT_DAY_COUNTS_SQL.contains("GROUP BY substr(ts_utc,1,10)"));
        assert!(!LIST_EVENT_DAY_COUNTS_SQL.contains(&alias_grouping));
    }

    #[test]
    fn delete_events_batch_query_orders_by_range_index_prefix() {
        let id_only_order = ["ORDER BY ", "id ASC"].concat();
        assert!(DELETE_EVENTS_IN_RANGE_BATCH_SQL.contains("WHERE ts_utc >= ? AND ts_utc < ?"));
        assert!(DELETE_EVENTS_IN_RANGE_BATCH_SQL.contains("ORDER BY ts_utc ASC, id ASC"));
        assert!(!DELETE_EVENTS_IN_RANGE_BATCH_SQL.contains(&id_only_order));
    }
}

impl StatsStorage {
    pub async fn insert_events(&self, events: &[EventInsert]) -> Result<(), AppError> {
        if events.is_empty() {
            return Ok(());
        }
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| AppError::Internal(format!("begin tx: {e}")))?;

        // SQLite 默认 `SQLITE_MAX_VARIABLE_NUMBER=999`，这里每行 11 列绑定参数，保守按 90 行/批次拆分。
        // 性能优化：从“逐条 INSERT”改为“单语句多行 INSERT”，降低 SQL 解析与往返开销，不改变写入语义。
        const COLS: usize = 11;
        const SQLITE_MAX_VARS: usize = 999;
        const MAX_ROWS_PER_INSERT: usize = SQLITE_MAX_VARS / COLS; // 90

        for chunk in events.chunks(MAX_ROWS_PER_INSERT) {
            let mut qb = QueryBuilder::<Sqlite>::new(
                "INSERT INTO events(ts_utc, route, feature, action, method, status, duration_ms, user_hash, client_ip_hash, instance, extra_json) ",
            );
            qb.push_values(chunk, |mut b, e| {
                let extra = e
                    .extra_json
                    .as_ref()
                    .map(|v| serde_json::to_string(v).unwrap_or_default());
                b.push_bind(e.ts_utc.to_rfc3339())
                    .push_bind(&e.route)
                    .push_bind(&e.feature)
                    .push_bind(&e.action)
                    .push_bind(&e.method)
                    .push_bind(e.status.map(i64::from))
                    .push_bind(e.duration_ms)
                    .push_bind(&e.user_hash)
                    .push_bind(&e.client_ip_hash)
                    .push_bind(e.instance.as_deref())
                    .push_bind(extra);
            });
            qb.build()
                .execute(&mut *tx)
                .await
                .map_err(|e| AppError::Internal(format!("insert event: {e}")))?;
        }
        tx.commit()
            .await
            .map_err(|e| AppError::Internal(format!("commit: {e}")))?;
        Ok(())
    }

    pub async fn checkpoint_wal_truncate(&self) -> Result<(), AppError> {
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE);")
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("wal checkpoint: {e}")))?;
        Ok(())
    }

    pub async fn list_event_day_counts(&self) -> Result<Vec<(String, i64)>, AppError> {
        let rows = sqlx::query(LIST_EVENT_DAY_COUNTS_SQL)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("load daily counts: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let day: String = r
                .try_get("day")
                .map_err(|e| AppError::Internal(format!("read day: {e}")))?;
            let count: i64 = r
                .try_get("c")
                .map_err(|e| AppError::Internal(format!("read day count: {e}")))?;
            out.push((day, count));
        }
        Ok(out)
    }

    pub async fn delete_events_in_range_batch(
        &self,
        from_rfc3339: &str,
        to_rfc3339: &str,
        batch_size: i64,
    ) -> Result<i64, AppError> {
        let res = sqlx::query(DELETE_EVENTS_IN_RANGE_BATCH_SQL)
            .bind(from_rfc3339)
            .bind(to_rfc3339)
            .bind(batch_size)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("delete events range: {e}")))?;
        Ok(saturating_u64_to_i64(res.rows_affected()))
    }

    pub async fn count_events_in_range(
        &self,
        from_rfc3339: &str,
        to_rfc3339: &str,
    ) -> Result<i64, AppError> {
        let row = sqlx::query("SELECT COUNT(1) as c FROM events WHERE ts_utc >= ? AND ts_utc < ?")
            .bind(from_rfc3339)
            .bind(to_rfc3339)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("count events range: {e}")))?;
        row.try_get::<i64, _>("c")
            .map_err(|e| AppError::Internal(format!("read events range count: {e}")))
    }

    pub async fn query_archive_events_between(
        &self,
        start_rfc3339: &str,
        end_rfc3339: &str,
    ) -> Result<Vec<ArchiveEventRow>, AppError> {
        let rows = sqlx::query(
            r"SELECT ts_utc, route, feature, action, method, status, duration_ms, user_hash, client_ip_hash, instance, extra_json FROM events WHERE ts_utc BETWEEN ? AND ? ORDER BY ts_utc ASC",
        )
        .bind(start_rfc3339)
        .bind(end_rfc3339)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("archive query: {e}")))?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(ArchiveEventRow {
                ts_utc: row.try_get::<String, _>("ts_utc").unwrap_or_default(),
                route: row.try_get::<String, _>("route").ok(),
                feature: row.try_get::<String, _>("feature").ok(),
                action: row.try_get::<String, _>("action").ok(),
                method: row.try_get::<String, _>("method").ok(),
                status: row.try_get::<i64, _>("status").ok(),
                duration_ms: row.try_get::<i64, _>("duration_ms").ok(),
                user_hash: row.try_get::<String, _>("user_hash").ok(),
                client_ip_hash: row.try_get::<String, _>("client_ip_hash").ok(),
                instance: row.try_get::<String, _>("instance").ok(),
                extra_json: row.try_get::<String, _>("extra_json").ok(),
            });
        }
        Ok(out)
    }
}
