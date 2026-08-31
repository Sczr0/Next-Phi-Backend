//! LeaderboardRepo 端口实现（Phase 2 Step 3）——StatsStorage 适配层次。
//!
//! 现有只读查询方法逐方法委托；错误经 `app_err_to_storage` 映射为领域错误
//! （原始 `AppError` 已带上下文并先落日志于存储层）。
//! 契约测试：`leaderboard_repo_contract_suite`（phi-contract）在本 crate 以
//! 真 SQLite 运行——fake 与真实现必须通过同一套件。

use async_trait::async_trait;
use phi_contract::{
    error::StorageError,
    repo::LeaderboardRepo,
    storage::{
        AdminLeaderboardUserRow, LeaderboardDetailsRow, LeaderboardTopRow, ModerationStateFullRow,
        PublicProfileRow, SuspiciousRow,
    },
};

use crate::error::AppError;

fn app_err_to_storage(e: AppError) -> StorageError {
    match e {
        AppError::Internal(msg) => StorageError::Internal(msg),
        other => StorageError::Internal(other.to_string()),
    }
}

#[async_trait]
impl LeaderboardRepo for crate::storage::StatsStorage {
    async fn count_public_total(&self) -> Result<i64, StorageError> {
        self.count_public_leaderboard_total()
            .await
            .map_err(app_err_to_storage)
    }

    async fn top_offset(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<LeaderboardTopRow>, StorageError> {
        self.query_leaderboard_top_offset(limit, offset)
            .await
            .map_err(app_err_to_storage)
    }

    async fn top_seek(
        &self,
        after_score: f64,
        after_updated: &str,
        after_user: &str,
        limit: i64,
    ) -> Result<Vec<LeaderboardTopRow>, StorageError> {
        self.query_leaderboard_top_seek(after_score, after_updated, after_user, limit)
            .await
            .map_err(app_err_to_storage)
    }

    async fn by_rank(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<LeaderboardTopRow>, StorageError> {
        self.query_leaderboard_by_rank(limit, offset)
            .await
            .map_err(app_err_to_storage)
    }

    async fn public_profile_by_alias(
        &self,
        alias: &str,
    ) -> Result<Option<PublicProfileRow>, StorageError> {
        self.query_public_profile_by_alias(alias)
            .await
            .map_err(app_err_to_storage)
    }

    async fn details_row(
        &self,
        user_hash: &str,
    ) -> Result<Option<LeaderboardDetailsRow>, StorageError> {
        self.query_leaderboard_details_row(user_hash)
            .await
            .map_err(app_err_to_storage)
    }

    async fn suspicious_rows(
        &self,
        min_score: f64,
        limit: i64,
    ) -> Result<Vec<SuspiciousRow>, StorageError> {
        self.query_suspicious_rows(min_score, limit)
            .await
            .map_err(app_err_to_storage)
    }

    async fn admin_users_rows(
        &self,
        status_filter: Option<&str>,
        alias_like: Option<&str>,
        page_size: i64,
        offset: i64,
    ) -> Result<Vec<AdminLeaderboardUserRow>, StorageError> {
        self.query_admin_leaderboard_users_rows(status_filter, alias_like, page_size, offset)
            .await
            .map_err(app_err_to_storage)
    }

    async fn user_moderation_full_row(
        &self,
        user_hash: &str,
    ) -> Result<Option<ModerationStateFullRow>, StorageError> {
        self.query_user_moderation_state_full_row(user_hash)
            .await
            .map_err(app_err_to_storage)
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;
    use phi_contract::repo::leaderboard_repo_contract_suite;

    async fn seeded_storage() -> crate::storage::StatsStorage {
        let path = std::env::temp_dir().join(format!(
            "phi_stats_contract_test_{}.db",
            uuid::Uuid::new_v4()
        ));
        let storage =
            crate::storage::StatsStorage::connect_sqlite(path.to_string_lossy().as_ref(), false)
                .await
                .expect("connect");
        storage.init_schema().await.expect("schema");

        let seed_rows = [
            ("apple", 15.0_f64, 0.10_f64),
            ("orange", 14.0, 0.20),
            ("pear", 13.0, 0.30),
            ("hidden", 16.0, 0.80),
            ("banned", 12.0, 0.90),
        ];
        for (h, rks, sus) in seed_rows {
            sqlx::query(
                "INSERT INTO leaderboard_rks (user_hash, total_rks, suspicion_score, is_hidden, created_at, updated_at)
                 VALUES (?, ?, ?, ?, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            )
            .bind(h)
            .bind(rks)
            .bind(sus)
            .bind(i64::from(h == "hidden"))
            .execute(&storage.pool)
            .await
            .expect("seed rks");
        }
        let seed_profiles = [
            ("apple", "apples", 1_i64),
            ("orange", "oranges", 1),
            ("pear", "pears", 1),
            ("hidden", "hidden-u", 0),
            ("banned", "banned-u", 0),
        ];
        for (h, alias, is_public) in seed_profiles {
            sqlx::query(
                "INSERT INTO user_profile (user_hash, alias, is_public, show_rks_composition, show_best_top3, show_ap_top3, created_at, updated_at)
                 VALUES (?, ?, ?, 1, 0, 0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            )
            .bind(h)
            .bind(alias)
            .bind(is_public)
            .execute(&storage.pool)
            .await
            .expect("seed profile");
        }
        sqlx::query(
            "INSERT INTO leaderboard_details (user_hash, best_top3_json, ap_top3_json, updated_at)
             VALUES ('apple', '[]', '[]', '2026-01-01T00:00:00Z')",
        )
        .execute(&storage.pool)
        .await
        .expect("seed details");
        sqlx::query(
            "INSERT INTO user_moderation_state (user_hash, status, reason, updated_by, updated_at)
             VALUES ('banned', 'banned', 'spam', 'admin', '2026-01-01T00:00:00Z')",
        )
        .execute(&storage.pool)
        .await
        .expect("seed mod state");

        let _ = std::fs::remove_file(&path);
        storage
    }

    #[tokio::test]
    async fn sqlite_impl_passes_contract_suite() {
        let storage = seeded_storage().await;
        leaderboard_repo_contract_suite(&storage)
            .await
            .expect("真 SQLite 实现应通过契约套件");
    }
}
