use sqlx::Row;

use crate::error::AppError;

use super::{RksHistoryCursor, RksHistoryEntry, RksHistoryPage, StatsStorage, SubmissionRecord};

// 归一化浮点噪声：避免把 1e-15 量级差值当成“RKS 变化”暴露给客户端。
const RKS_JUMP_EPS: f64 = 1e-9;
const PEAK_RKS_SQL: &str = "SELECT total_rks as peak FROM save_submissions WHERE user_hash = ? ORDER BY total_rks DESC LIMIT 1";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peak_rks_query_uses_ordered_limit_instead_of_aggregate_scan() {
        let aggregate_fn = ["MAX", "("].concat();
        assert!(PEAK_RKS_SQL.contains("ORDER BY total_rks DESC LIMIT 1"));
        assert!(!PEAK_RKS_SQL.contains(&aggregate_fn));
    }
}

#[allow(clippy::needless_pass_by_value)]
fn row_to_rks_history_entry(row: sqlx::sqlite::SqliteRow) -> RksHistoryEntry {
    let rks = row.try_get::<f64, _>("total_rks").unwrap_or(0.0);
    let rks_jump = row.try_get::<f64, _>("rks_jump").unwrap_or(0.0);
    let rks_jump = if rks_jump.abs() < RKS_JUMP_EPS {
        0.0
    } else {
        rks_jump
    };
    RksHistoryEntry {
        id: row.try_get::<i64, _>("id").unwrap_or(0),
        rks,
        rks_jump,
        created_at: row.try_get::<String, _>("created_at").unwrap_or_default(),
    }
}

impl StatsStorage {
    pub async fn insert_submission(&self, record: SubmissionRecord<'_>) -> Result<(), AppError> {
        let SubmissionRecord {
            user_hash,
            total_rks,
            rks_jump,
            route,
            client_ip_hash,
            details_json,
            suspicion_score,
            now_rfc3339,
        } = record;
        sqlx::query("INSERT INTO save_submissions(user_hash,total_rks,acc_stats,rks_jump,route,client_ip_hash,details_json,suspicion_score,created_at) VALUES(?,?,?,?,?,?,?,?,?)")
            .bind(user_hash)
            .bind(total_rks)
            .bind(Option::<String>::None)
            .bind(rks_jump)
            .bind(route)
            .bind(client_ip_hash)
            .bind(details_json)
            .bind(suspicion_score)
            .bind(now_rfc3339)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("insert submission: {e}")))?;
        Ok(())
    }

    /// 查询用户 RKS 历史记录
    ///
    /// 返回 (历史记录列表, 总记录数)
    pub async fn query_rks_history(
        &self,
        user_hash: &str,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<RksHistoryEntry>, i64), AppError> {
        let page = self
            .query_rks_history_page(user_hash, limit, offset, None)
            .await?;
        Ok((page.entries, page.total))
    }

    /// 查询用户 RKS 历史页。
    ///
    /// `cursor` 存在时使用 `(created_at, id)` seek 分页；否则保留旧 offset 语义。
    #[allow(clippy::cast_sign_loss)]
    pub async fn query_rks_history_page(
        &self,
        user_hash: &str,
        limit: i64,
        offset: i64,
        cursor: Option<&RksHistoryCursor>,
    ) -> Result<RksHistoryPage, AppError> {
        let limit = limit.clamp(1, 500);
        let offset = offset.max(0);
        let count_fut = async {
            let count_row =
                sqlx::query("SELECT COUNT(1) as c FROM save_submissions WHERE user_hash = ?")
                    .bind(user_hash)
                    .fetch_one(&self.pool)
                    .await
                    .map_err(|e| AppError::Internal(format!("count rks history: {e}")))?;
            Ok::<i64, AppError>(count_row.try_get("c").unwrap_or(0))
        };

        let fetch_limit = limit.saturating_add(1);
        let rows_fut = async {
            if let Some(cursor) = cursor {
                sqlx::query(
                    "SELECT id, total_rks, rks_jump, created_at
                     FROM save_submissions
                     WHERE user_hash = ?
                       AND (created_at < ? OR (created_at = ? AND id < ?))
                     ORDER BY created_at DESC, id DESC
                     LIMIT ?",
                )
                .bind(user_hash)
                .bind(&cursor.created_at)
                .bind(&cursor.created_at)
                .bind(cursor.id)
                .bind(fetch_limit)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("query rks history cursor: {e}")))
            } else {
                // 旧 offset 分页保留兼容；排序补上 id，避免相同 created_at 下分页顺序漂移。
                sqlx::query(
                    "SELECT id, total_rks, rks_jump, created_at
                     FROM save_submissions
                     WHERE user_hash = ?
                     ORDER BY created_at DESC, id DESC
                     LIMIT ? OFFSET ?",
                )
                .bind(user_hash)
                .bind(fetch_limit)
                .bind(offset)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("query rks history: {e}")))
            }
        };

        let (total, rows) = tokio::try_join!(count_fut, rows_fut)?;
        let mut entries: Vec<RksHistoryEntry> =
            rows.into_iter().map(row_to_rks_history_entry).collect();
        let has_more = entries.len() > limit as usize;
        if has_more {
            entries.truncate(limit as usize);
        }

        Ok(RksHistoryPage {
            entries,
            total,
            has_more,
        })
    }

    /// 获取用户历史最高 RKS
    pub async fn get_peak_rks(&self, user_hash: &str) -> Result<f64, AppError> {
        let row = sqlx::query(PEAK_RKS_SQL)
            .bind(user_hash)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("get peak rks: {e}")))?;

        Ok(row
            .and_then(|row| row.try_get::<f64, _>("peak").ok())
            .unwrap_or(0.0))
    }
}
