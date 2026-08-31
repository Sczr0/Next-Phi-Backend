use uuid::Uuid;

use crate::error::AppError;

use super::rows::row_to_developer;
use super::{
    DeveloperRecord, OpenPlatformStorage, SELECT_DEVELOPER_BY_GITHUB_USER_ID,
    SELECT_DEVELOPER_BY_ID,
};

impl OpenPlatformStorage {
    pub async fn upsert_developer_by_github(
        &self,
        github_user_id: &str,
        github_login: &str,
        email: Option<&str>,
        now_ts: i64,
    ) -> Result<DeveloperRecord, AppError> {
        let developer_id = format!("dev_{}", Uuid::new_v4().simple());
        sqlx::query(
            "INSERT INTO developers(id, github_user_id, github_login, email, role, status, created_at, updated_at)
             VALUES(?, ?, ?, ?, 'developer', 'active', ?, ?)
             ON CONFLICT(github_user_id) DO UPDATE SET
               github_login = excluded.github_login,
               email = excluded.email,
               updated_at = excluded.updated_at",
        )
        .bind(&developer_id)
        .bind(github_user_id)
        .bind(github_login)
        .bind(email)
        .bind(now_ts)
        .bind(now_ts)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("upsert developer: {e}")))?;

        let row = sqlx::query(SELECT_DEVELOPER_BY_GITHUB_USER_ID)
            .bind(github_user_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("query developer after upsert: {e}")))?;
        Ok(row_to_developer(&row))
    }

    pub async fn get_developer_by_id(
        &self,
        developer_id: &str,
    ) -> Result<Option<DeveloperRecord>, AppError> {
        let row = sqlx::query(SELECT_DEVELOPER_BY_ID)
            .bind(developer_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("query developer by id: {e}")))?;
        Ok(row.as_ref().map(row_to_developer))
    }

    pub async fn get_developer_by_github_user_id(
        &self,
        github_user_id: &str,
    ) -> Result<Option<DeveloperRecord>, AppError> {
        let row = sqlx::query(SELECT_DEVELOPER_BY_GITHUB_USER_ID)
            .bind(github_user_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("query developer by github user id: {e}")))?;
        Ok(row.as_ref().map(row_to_developer))
    }
}
