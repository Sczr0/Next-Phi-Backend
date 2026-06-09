use sqlx::{QueryBuilder, Row, Sqlite, sqlite::SqliteRow};

use crate::error::AppError;

use super::StatsStorage;

fn push_admin_status_filter(qb: &mut QueryBuilder<'_, Sqlite>, status: &str) {
    if status.eq_ignore_ascii_case("active") {
        qb.push(" AND (ums.status IS NULL OR ums.status = 'active' COLLATE NOCASE)");
    } else {
        qb.push(" AND ums.status = ")
            .push_bind(status.to_string())
            .push(" COLLATE NOCASE");
    }
}

fn non_active_status_filter(status_filter: Option<&str>) -> Option<&str> {
    status_filter.filter(|status| !status.eq_ignore_ascii_case("active"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::Execute as _;

    #[test]
    fn admin_status_filter_keeps_active_null_semantics() {
        let mut qb = QueryBuilder::<Sqlite>::new("WHERE 1=1");
        push_admin_status_filter(&mut qb, "active");
        let sql = qb.build().sql().to_string();

        assert!(sql.contains("ums.status IS NULL OR ums.status = 'active' COLLATE NOCASE"));
        assert!(!sql.contains("LOWER("));
        assert!(!sql.contains("COALESCE("));
    }

    #[test]
    fn admin_status_filter_uses_direct_predicate_for_non_active() {
        let mut qb = QueryBuilder::<Sqlite>::new("WHERE 1=1");
        push_admin_status_filter(&mut qb, "banned");
        let sql = qb.build().sql().to_string();

        assert!(sql.contains("ums.status = ? COLLATE NOCASE"));
        assert!(!sql.contains("LOWER("));
        assert!(!sql.contains("COALESCE("));
    }
}

impl StatsStorage {
    pub async fn query_suspicious_rows(
        &self,
        min_score: f64,
        limit: i64,
    ) -> Result<Vec<SqliteRow>, AppError> {
        sqlx::query(
            "SELECT lr.user_hash, lr.total_rks, lr.suspicion_score, lr.updated_at, up.alias
             FROM leaderboard_rks lr LEFT JOIN user_profile up ON up.user_hash=lr.user_hash
             WHERE lr.suspicion_score >= ?
             ORDER BY lr.suspicion_score DESC, lr.total_rks DESC, lr.user_hash ASC
             LIMIT ?",
        )
        .bind(min_score)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("query suspicious rows: {e}")))
    }

    pub async fn query_admin_leaderboard_users_count(
        &self,
        status_filter: Option<&str>,
        alias_like: Option<&str>,
    ) -> Result<i64, AppError> {
        let mut count_qb = if let Some(status) = non_active_status_filter(status_filter) {
            let mut qb = QueryBuilder::<Sqlite>::new(
                "SELECT COUNT(1) AS c
                 FROM user_moderation_state ums
                 JOIN leaderboard_rks lr ON lr.user_hash=ums.user_hash
                 LEFT JOIN user_profile up ON up.user_hash=lr.user_hash
                 WHERE ums.status = ",
            );
            qb.push_bind(status.to_string()).push(" COLLATE NOCASE");
            qb
        } else {
            let mut qb = QueryBuilder::<Sqlite>::new(
                "SELECT COUNT(1) AS c
                 FROM leaderboard_rks lr
                 LEFT JOIN user_profile up ON up.user_hash=lr.user_hash
                 LEFT JOIN user_moderation_state ums ON ums.user_hash=lr.user_hash
                 WHERE 1=1",
            );
            if let Some(status) = status_filter {
                push_admin_status_filter(&mut qb, status);
            }
            qb
        };
        if let Some(alias) = alias_like {
            count_qb
                .push(" AND up.alias LIKE ")
                .push_bind(alias.to_string());
        }
        let row = count_qb
            .build()
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("admin users count: {e}")))?;
        Ok(row.try_get("c").unwrap_or(0))
    }

    pub async fn query_admin_leaderboard_users_rows(
        &self,
        status_filter: Option<&str>,
        alias_like: Option<&str>,
        page_size: i64,
        offset: i64,
    ) -> Result<Vec<SqliteRow>, AppError> {
        let mut qb = if let Some(status) = non_active_status_filter(status_filter) {
            let mut qb = QueryBuilder::<Sqlite>::new(
                "SELECT
                    lr.user_hash,
                    up.alias,
                    lr.total_rks,
                    lr.suspicion_score,
                    lr.is_hidden,
                    lr.updated_at,
                    ums.status AS status
                 FROM user_moderation_state ums
                 JOIN leaderboard_rks lr ON lr.user_hash=ums.user_hash
                 LEFT JOIN user_profile up ON up.user_hash=lr.user_hash
                 WHERE ums.status = ",
            );
            qb.push_bind(status.to_string()).push(" COLLATE NOCASE");
            qb
        } else {
            let mut qb = QueryBuilder::<Sqlite>::new(
                "SELECT
                    lr.user_hash,
                    up.alias,
                    lr.total_rks,
                    lr.suspicion_score,
                    lr.is_hidden,
                    lr.updated_at,
                    COALESCE(ums.status, 'active') AS status
                 FROM leaderboard_rks lr
                 LEFT JOIN user_profile up ON up.user_hash=lr.user_hash
                 LEFT JOIN user_moderation_state ums ON ums.user_hash=lr.user_hash
                 WHERE 1=1",
            );
            if let Some(status) = status_filter {
                push_admin_status_filter(&mut qb, status);
            }
            qb
        };
        if let Some(alias) = alias_like {
            qb.push(" AND up.alias LIKE ").push_bind(alias.to_string());
        }
        qb.push(" ORDER BY lr.total_rks DESC, lr.updated_at ASC, lr.user_hash ASC");
        qb.push(" LIMIT ").push_bind(page_size);
        qb.push(" OFFSET ").push_bind(offset);
        qb.build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("admin users list: {e}")))
    }

    pub async fn query_user_moderation_state_full_row(
        &self,
        user_hash: &str,
    ) -> Result<Option<SqliteRow>, AppError> {
        sqlx::query(
            "SELECT status, reason, updated_by, updated_at
             FROM user_moderation_state
             WHERE user_hash = ?
             LIMIT 1",
        )
        .bind(user_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("query user moderation full row: {e}")))
    }

    fn build_banned_detail(reason: Option<&str>) -> String {
        if let Some(r) = reason.map(str::trim).filter(|v| !v.is_empty()) {
            return format!("用户已被全局封禁，原因：{r}");
        }
        "用户已被全局封禁".to_string()
    }

    pub async fn get_user_moderation_state(
        &self,
        user_hash: &str,
    ) -> Result<Option<(String, Option<String>)>, AppError> {
        let row = sqlx::query(
            "SELECT status, reason FROM user_moderation_state WHERE user_hash = ? LIMIT 1",
        )
        .bind(user_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("query moderation state: {e}")))?;

        let Some(r) = row else {
            return Ok(None);
        };
        let status = r
            .try_get::<String, _>("status")
            .unwrap_or_else(|_| "active".to_string());
        let reason = r
            .try_get::<Option<String>, _>("reason")
            .unwrap_or(None)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        Ok(Some((status, reason)))
    }

    pub async fn get_user_moderation_status(
        &self,
        user_hash: &str,
    ) -> Result<Option<String>, AppError> {
        Ok(self
            .get_user_moderation_state(user_hash)
            .await?
            .map(|(status, _)| status))
    }

    pub async fn ensure_user_not_banned(&self, user_hash: &str) -> Result<(), AppError> {
        if let Some((status, reason)) = self.get_user_moderation_state(user_hash).await?
            && status.eq_ignore_ascii_case("banned")
        {
            return Err(AppError::Forbidden(Self::build_banned_detail(
                reason.as_deref(),
            )));
        }
        Ok(())
    }

    pub async fn set_user_moderation_status(
        &self,
        user_hash: &str,
        status: &str,
        reason: Option<&str>,
        updated_by: &str,
        updated_at: &str,
    ) -> Result<(), AppError> {
        let reason_clean = reason.map(str::trim).filter(|v| !v.is_empty());
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| AppError::Internal(format!("moderation tx begin: {e}")))?;
        sqlx::query(
            "INSERT INTO user_moderation_state(user_hash,status,reason,updated_by,updated_at,expires_at)
             VALUES(?,?,?,?,?,NULL)
             ON CONFLICT(user_hash) DO UPDATE SET
               status = excluded.status,
               reason = excluded.reason,
               updated_by = excluded.updated_by,
               updated_at = excluded.updated_at,
               expires_at = NULL",
        )
        .bind(user_hash)
        .bind(status)
        .bind(reason_clean)
        .bind(updated_by)
        .bind(updated_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(format!("upsert moderation status: {e}")))?;

        sqlx::query(
            "INSERT INTO moderation_flags(user_hash,status,reason,severity,created_by,created_at)
             VALUES(?,?,?,?,?,?)",
        )
        .bind(user_hash)
        .bind(status)
        .bind(reason_clean.unwrap_or(""))
        .bind(0_i64)
        .bind(updated_by)
        .bind(updated_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(format!("insert moderation flag: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| AppError::Internal(format!("moderation tx commit: {e}")))?;
        Ok(())
    }
}
