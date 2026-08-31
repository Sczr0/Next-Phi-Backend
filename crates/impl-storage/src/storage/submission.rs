#![allow(clippy::items_after_test_module)]

use sqlx::Row;

use crate::error::AppError;

use super::{RksHistoryCursor, RksHistoryEntry, RksHistoryPage, StatsStorage, SubmissionRecord};

// 归一化浮点噪声：避免把 1e-15 量级差值当成“RKS 变化”暴露给客户端。
const RKS_JUMP_EPS: f64 = 1e-9;
const PEAK_RKS_SQL: &str = "SELECT total_rks as peak FROM save_submissions WHERE user_hash = ? ORDER BY total_rks DESC LIMIT 1";

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn peak_rks_query_uses_ordered_limit_instead_of_aggregate_scan() {
        let aggregate_fn = ["MAX", "("].concat();
        assert!(PEAK_RKS_SQL.contains("ORDER BY total_rks DESC LIMIT 1"));
        assert!(!PEAK_RKS_SQL.contains(&aggregate_fn));
    }

    /// D5 opt-in：每用户保留最近 N 条，更旧删除；keep=0 不清理。
    #[tokio::test]
    async fn trim_save_submissions_keeps_newest_per_user() {
        let path = std::env::temp_dir().join(format!("phi_submission_trim_{}.db", Uuid::new_v4()));
        let storage = StatsStorage::connect_sqlite(path.to_string_lossy().as_ref(), false)
            .await
            .expect("connect");
        storage.init_schema().await.expect("schema");

        for user in ["u1", "u2"] {
            for i in 0..5 {
                let created = format!("2026-01-0{}T00:00:00Z", i + 1);
                sqlx::query(
                    "INSERT INTO save_submissions (user_hash, total_rks, suspicion_score, created_at)
                     VALUES (?, ?, 0.0, ?)",
                )
                .bind(user)
                .bind(13.0 + f64::from(i))
                .bind(created)
                .execute(&storage.pool)
                .await
                .expect("seed");
            }
        }

        // keep=0 -> 不清理
        assert_eq!(
            storage
                .trim_save_submissions_per_user(0, 5000)
                .await
                .unwrap(),
            0
        );

        // keep=2 -> 每用户保留最近 2 条（各删 3 条，共 6）
        let deleted = storage
            .trim_save_submissions_per_user(2, 5000)
            .await
            .expect("trim");
        assert_eq!(deleted, 6);

        for user in ["u1", "u2"] {
            let count: i64 =
                sqlx::query_scalar("SELECT COUNT(1) FROM save_submissions WHERE user_hash = ?")
                    .bind(user)
                    .fetch_one(&storage.pool)
                    .await
                    .expect("count");
            assert_eq!(count, 2, "{user} 应保留 2 条");
            // 保留的应是最新两条（i=4, i=3 -> created_at 2026-01-05/04）
            let newest: String = sqlx::query_scalar(
                "SELECT created_at FROM save_submissions WHERE user_hash = ? ORDER BY created_at DESC LIMIT 1",
            )
            .bind(user)
            .fetch_one(&storage.pool)
            .await
            .expect("newest");
            assert_eq!(newest, "2026-01-05T00:00:00Z");
        }

        let _ = std::fs::remove_file(&path);
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
    /// D5 保留策略（opt-in，默认 0 不启用）：每个用户仅保留最近 `keep` 条
    /// `save_submissions`，更旧的按 `(created_at DESC, id DESC)` 序删除。
    ///
    /// - 窗口函数 ROW_NUMBER() OVER (PARTITION BY user_hash ...) 要求 SQLite >= 3.25；
    /// - 分批（内层 LIMIT）避免长事务锁写；返回累计删除行数；
    /// - 影响：RKS 历史接口（/rks/history）可回溯长度收缩（见 ADR-0003）。
    pub async fn trim_save_submissions_per_user(
        &self,
        keep: u32,
        batch_size: i64,
    ) -> Result<i64, AppError> {
        if keep == 0 {
            return Ok(0);
        }
        let mut total_deleted = 0i64;
        loop {
            let res = sqlx::query(
                "DELETE FROM save_submissions WHERE id IN (
                   SELECT id FROM (
                     SELECT id,
                            ROW_NUMBER() OVER (
                              PARTITION BY user_hash
                              ORDER BY created_at DESC, id DESC
                            ) AS rn
                     FROM save_submissions
                   ) WHERE rn > ?
                   LIMIT ?
                 )",
            )
            .bind(i64::from(keep))
            .bind(batch_size)
            .execute(&self.state_pool)
            .await
            .map_err(|e| AppError::Internal(format!("trim save_submissions: {e}")))?;
            let affected = i64::try_from(res.rows_affected()).unwrap_or(i64::MAX);
            total_deleted += affected;
            if affected < batch_size {
                break;
            }
        }
        Ok(total_deleted)
    }

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
            .execute(&self.state_pool)
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
                    .fetch_one(&self.state_pool)
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
                .fetch_all(&self.state_pool)
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
                .fetch_all(&self.state_pool)
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
            .fetch_optional(&self.state_pool)
            .await
            .map_err(|e| AppError::Internal(format!("get peak rks: {e}")))?;

        Ok(row
            .and_then(|row| row.try_get::<f64, _>("peak").ok())
            .unwrap_or(0.0))
    }
}
