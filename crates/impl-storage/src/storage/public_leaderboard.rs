#![allow(clippy::items_after_test_module)]

use sqlx::{QueryBuilder, Row, Sqlite, sqlite::SqliteRow};

use crate::error::AppError;

use super::StatsStorage;

const COUNT_PUBLIC_LEADERBOARD_TOTAL_SQL: &str = "SELECT COUNT(1) AS c
             FROM leaderboard_rks lr JOIN user_profile up ON up.user_hash=lr.user_hash AND up.is_public=1
             WHERE lr.is_hidden=0";

const QUERY_LEADERBOARD_TOP_SEEK_SQL: &str =
    "SELECT lr.user_hash, lr.total_rks, lr.updated_at, up.alias, COALESCE(up.show_best_top3,0) AS sbt, COALESCE(up.show_ap_top3,0) AS sat
             FROM leaderboard_rks lr JOIN user_profile up ON up.user_hash=lr.user_hash AND up.is_public=1
             WHERE lr.is_hidden=0 AND (
               lr.total_rks < ? OR (lr.total_rks = ? AND (lr.updated_at > ? OR (lr.updated_at = ? AND lr.user_hash > ?)))
             )
             ORDER BY lr.total_rks DESC, lr.updated_at ASC, lr.user_hash ASC
             LIMIT ?";

const QUERY_LEADERBOARD_TOP_OFFSET_SQL: &str =
    "SELECT lr.user_hash, lr.total_rks, lr.updated_at, up.alias, COALESCE(up.show_best_top3,0) AS sbt, COALESCE(up.show_ap_top3,0) AS sat
             FROM leaderboard_rks lr JOIN user_profile up ON up.user_hash=lr.user_hash AND up.is_public=1
             WHERE lr.is_hidden=0
             ORDER BY lr.total_rks DESC, lr.updated_at ASC, lr.user_hash ASC
             LIMIT ? OFFSET ?";

const COUNT_PUBLIC_LEADERBOARD_HIGHER_SQL: &str =
    "SELECT COUNT(1) as higher FROM leaderboard_rks lr JOIN user_profile up ON up.user_hash=lr.user_hash AND up.is_public=1
             WHERE lr.is_hidden=0 AND (
               lr.total_rks > ? OR (lr.total_rks = ? AND (lr.updated_at < ? OR (lr.updated_at = ? AND lr.user_hash < ?)))
             )";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_leaderboard_join_uses_indexable_public_predicate() {
        let queries = [
            COUNT_PUBLIC_LEADERBOARD_TOTAL_SQL,
            QUERY_LEADERBOARD_TOP_SEEK_SQL,
            QUERY_LEADERBOARD_TOP_OFFSET_SQL,
            COUNT_PUBLIC_LEADERBOARD_HIGHER_SQL,
        ];

        let coalesce_public = ["COALESCE(", "up.is_public"].concat();
        let left_profile_join = ["LEFT JOIN ", "user_profile"].concat();
        for sql in queries {
            assert!(sql.contains("JOIN user_profile up"));
            assert!(sql.contains("up.is_public=1"));
            assert!(!sql.contains(&coalesce_public));
            assert!(!sql.contains(&left_profile_join));
        }
    }
}

impl StatsStorage {
    pub async fn count_public_leaderboard_total(&self) -> Result<i64, AppError> {
        let row = sqlx::query(COUNT_PUBLIC_LEADERBOARD_TOTAL_SQL)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("count public leaderboard total: {e}")))?;
        Ok(row.try_get("c").unwrap_or(0))
    }

    pub async fn query_leaderboard_top_seek(
        &self,
        after_score: f64,
        after_updated: &str,
        after_user: &str,
        limit: i64,
    ) -> Result<Vec<SqliteRow>, AppError> {
        sqlx::query(QUERY_LEADERBOARD_TOP_SEEK_SQL)
            .bind(after_score)
            .bind(after_score)
            .bind(after_updated)
            .bind(after_updated)
            .bind(after_user)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("query top seek: {e}")))
    }

    pub async fn query_leaderboard_top_offset(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<SqliteRow>, AppError> {
        sqlx::query(QUERY_LEADERBOARD_TOP_OFFSET_SQL)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("query top offset: {e}")))
    }

    pub async fn query_leaderboard_by_rank(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<SqliteRow>, AppError> {
        self.query_leaderboard_top_offset(limit, offset).await
    }

    pub async fn fetch_top3_details_for_users(
        &self,
        user_hashes: &[String],
    ) -> Result<std::collections::HashMap<String, (Option<String>, Option<String>)>, AppError> {
        if user_hashes.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let mut qb = QueryBuilder::<Sqlite>::new(
            "SELECT user_hash, best_top3_json, ap_top3_json FROM leaderboard_details WHERE user_hash IN (",
        );
        let mut separated = qb.separated(", ");
        for uh in user_hashes {
            separated.push_bind(uh);
        }
        qb.push(")");
        let rows = qb
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("fetch top3 details: {e}")))?;
        let mut map = std::collections::HashMap::with_capacity(rows.len());
        for r in rows {
            let user_hash = r.try_get::<String, _>("user_hash").unwrap_or_default();
            let best_json = r.try_get::<String, _>("best_top3_json").ok();
            let ap_json = r.try_get::<String, _>("ap_top3_json").ok();
            map.insert(user_hash, (best_json, ap_json));
        }
        Ok(map)
    }

    pub async fn count_public_leaderboard_higher(
        &self,
        score: f64,
        updated_at: &str,
        user_hash: &str,
    ) -> Result<i64, AppError> {
        let row = sqlx::query(COUNT_PUBLIC_LEADERBOARD_HIGHER_SQL)
            .bind(score)
            .bind(score)
            .bind(updated_at)
            .bind(updated_at)
            .bind(user_hash)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("count public leaderboard higher: {e}")))?;
        Ok(row.try_get("higher").unwrap_or(0))
    }
}
