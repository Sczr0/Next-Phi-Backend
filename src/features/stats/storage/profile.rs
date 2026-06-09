use sqlx::sqlite::SqliteRow;

use crate::error::AppError;

use super::{StatsStorage, UserAliasDefaults};

impl StatsStorage {
    pub async fn update_user_profile_visibility(
        &self,
        user_hash: &str,
        now_rfc3339: &str,
        is_public: Option<i64>,
        show_rks_composition: Option<i64>,
        show_best_top3: Option<i64>,
        show_ap_top3: Option<i64>,
    ) -> Result<(), AppError> {
        let mut sets: Vec<&str> = Vec::new();
        if is_public.is_some() {
            sets.push("is_public=?");
        }
        if show_rks_composition.is_some() {
            sets.push("show_rks_composition=?");
        }
        if show_best_top3.is_some() {
            sets.push("show_best_top3=?");
        }
        if show_ap_top3.is_some() {
            sets.push("show_ap_top3=?");
        }
        sets.push("updated_at=?");
        let sql = format!(
            "UPDATE user_profile SET {} WHERE user_hash=?",
            sets.join(",")
        );
        let mut q = sqlx::query(&sql);
        if let Some(v) = is_public {
            q = q.bind(v);
        }
        if let Some(v) = show_rks_composition {
            q = q.bind(v);
        }
        if let Some(v) = show_best_top3 {
            q = q.bind(v);
        }
        if let Some(v) = show_ap_top3 {
            q = q.bind(v);
        }
        q = q.bind(now_rfc3339).bind(user_hash);
        q.execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("update profile visibility: {e}")))?;
        Ok(())
    }

    pub async fn query_public_profile_by_alias(
        &self,
        alias: &str,
    ) -> Result<Option<SqliteRow>, AppError> {
        sqlx::query(
            "SELECT up.user_hash, up.is_public, up.show_rks_composition, up.show_best_top3, up.show_ap_top3, lr.total_rks, lr.updated_at
             FROM user_profile up LEFT JOIN leaderboard_rks lr ON lr.user_hash=up.user_hash WHERE up.alias = ?",
        )
        .bind(alias)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("query public profile by alias: {e}")))
    }

    pub async fn query_leaderboard_details_row(
        &self,
        user_hash: &str,
    ) -> Result<Option<SqliteRow>, AppError> {
        sqlx::query(
            "SELECT rks_composition_json, best_top3_json, ap_top3_json FROM leaderboard_details WHERE user_hash = ?",
        )
        .bind(user_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("query leaderboard details row: {e}")))
    }

    pub async fn ensure_default_public_profile(
        &self,
        user_hash: &str,
        user_kind: Option<&str>,
        show_rks_composition: bool,
        show_best_top3: bool,
        show_ap_top3: bool,
        now_rfc3339: &str,
    ) -> Result<(), AppError> {
        let show_rks_comp_i = i64::from(show_rks_composition);
        let show_best_top3_i = i64::from(show_best_top3);
        let show_ap_top3_i = i64::from(show_ap_top3);
        sqlx::query(
            "INSERT INTO user_profile(user_hash,is_public,show_rks_composition,show_best_top3,show_ap_top3,user_kind,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?)
             ON CONFLICT(user_hash) DO NOTHING",
        )
        .bind(user_hash)
        .bind(1_i64)
        .bind(show_rks_comp_i)
        .bind(show_best_top3_i)
        .bind(show_ap_top3_i)
        .bind(user_kind)
        .bind(now_rfc3339)
        .bind(now_rfc3339)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("ensure default public profile: {e}")))?;
        Ok(())
    }

    pub async fn ensure_user_profile_exists(
        &self,
        user_hash: &str,
        now_rfc3339: &str,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO user_profile(user_hash,created_at,updated_at) VALUES(?,?,?) ON CONFLICT(user_hash) DO NOTHING",
        )
        .bind(user_hash)
        .bind(now_rfc3339)
        .bind(now_rfc3339)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("ensure user profile exists: {e}")))?;
        Ok(())
    }

    pub async fn upsert_user_alias_with_defaults(
        &self,
        user_hash: &str,
        alias: &str,
        defaults: UserAliasDefaults<'_>,
    ) -> Result<(), AppError> {
        let is_public_i = i64::from(defaults.is_public);
        let show_rks_comp_i = i64::from(defaults.show_rks_composition);
        let show_best_top3_i = i64::from(defaults.show_best_top3);
        let show_ap_top3_i = i64::from(defaults.show_ap_top3);
        let res = sqlx::query(
            "INSERT INTO user_profile(user_hash,alias,is_public,show_rks_composition,show_best_top3,show_ap_top3,user_kind,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?)
             ON CONFLICT(user_hash) DO UPDATE SET alias=excluded.alias, updated_at=excluded.updated_at",
        )
        .bind(user_hash)
        .bind(alias)
        .bind(is_public_i)
        .bind(show_rks_comp_i)
        .bind(show_best_top3_i)
        .bind(show_ap_top3_i)
        .bind(Option::<String>::None)
        .bind(defaults.now_rfc3339)
        .bind(defaults.now_rfc3339)
        .execute(&self.pool)
        .await;
        match res {
            Ok(_) => Ok(()),
            Err(e) => {
                if e.to_string().to_lowercase().contains("unique") {
                    return Err(AppError::Conflict("别名已被占用".into()));
                }
                Err(AppError::Internal(format!("set alias failed: {e}")))
            }
        }
    }

    pub async fn force_set_user_alias(
        &self,
        user_hash: &str,
        alias: &str,
        now_rfc3339: &str,
    ) -> Result<(), AppError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| AppError::Internal(format!("tx begin: {e}")))?;
        sqlx::query("UPDATE user_profile SET alias=NULL, updated_at=? WHERE alias=?")
            .bind(now_rfc3339)
            .bind(alias)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::Internal(format!("clear alias: {e}")))?;
        sqlx::query(
            "INSERT INTO user_profile(user_hash,created_at,updated_at) VALUES(?,?,?) ON CONFLICT(user_hash) DO NOTHING",
        )
        .bind(user_hash)
        .bind(now_rfc3339)
        .bind(now_rfc3339)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(format!("ensure profile: {e}")))?;
        sqlx::query("UPDATE user_profile SET alias=?, updated_at=? WHERE user_hash=?")
            .bind(alias)
            .bind(now_rfc3339)
            .bind(user_hash)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::Internal(format!("set alias: {e}")))?;
        tx.commit()
            .await
            .map_err(|e| AppError::Internal(format!("tx commit: {e}")))?;
        Ok(())
    }
}
