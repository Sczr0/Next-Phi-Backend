use std::sync::atomic::{AtomicI64, Ordering};

use sqlx::Row;

use crate::error::AppError;

use super::StatsStorage;

const SESSION_CLEANUP_INTERVAL_SECS: i64 = 300;
static LAST_SESSION_CLEANUP_TS: AtomicI64 = AtomicI64::new(0);

impl StatsStorage {
    pub async fn add_token_blacklist(
        &self,
        jti: &str,
        expires_at: &str,
        created_at: &str,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO session_token_blacklist(jti,expires_at,created_at) VALUES(?,?,?)
             ON CONFLICT(jti) DO UPDATE SET expires_at=excluded.expires_at, created_at=excluded.created_at",
        )
        .bind(jti)
        .bind(expires_at)
        .bind(created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("insert session blacklist: {e}")))?;
        Ok(())
    }

    pub async fn is_token_blacklisted(
        &self,
        jti: &str,
        now_rfc3339: &str,
    ) -> Result<bool, AppError> {
        let row = sqlx::query(
            "SELECT 1 FROM session_token_blacklist WHERE jti = ? AND expires_at > ? LIMIT 1",
        )
        .bind(jti)
        .bind(now_rfc3339)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("query session blacklist: {e}")))?;
        Ok(row.is_some())
    }

    pub async fn upsert_logout_gate(
        &self,
        user_hash: &str,
        logout_before: &str,
        expires_at: &str,
        updated_at: &str,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO session_logout_gate(user_hash,logout_before,expires_at,updated_at) VALUES(?,?,?,?)
             ON CONFLICT(user_hash) DO UPDATE SET
               logout_before = excluded.logout_before,
               expires_at = excluded.expires_at,
               updated_at = excluded.updated_at",
        )
        .bind(user_hash)
        .bind(logout_before)
        .bind(expires_at)
        .bind(updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("upsert session logout gate: {e}")))?;
        Ok(())
    }

    pub async fn get_logout_gate(
        &self,
        user_hash: &str,
        now_rfc3339: &str,
    ) -> Result<Option<String>, AppError> {
        let row = sqlx::query(
            "SELECT logout_before FROM session_logout_gate WHERE user_hash = ? AND expires_at > ? LIMIT 1",
        )
        .bind(user_hash)
        .bind(now_rfc3339)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("query session logout gate: {e}")))?;
        Ok(row.and_then(|r| r.try_get::<String, _>("logout_before").ok()))
    }

    pub async fn get_session_revoke_state(
        &self,
        jti: &str,
        user_hash: &str,
        now_rfc3339: &str,
    ) -> Result<(bool, Option<String>), AppError> {
        let row = sqlx::query(
            "SELECT
               EXISTS(SELECT 1 FROM session_token_blacklist WHERE jti = ? AND expires_at > ?) AS blacklisted,
               (SELECT logout_before FROM session_logout_gate WHERE user_hash = ? AND expires_at > ? LIMIT 1) AS logout_before",
        )
        .bind(jti)
        .bind(now_rfc3339)
        .bind(user_hash)
        .bind(now_rfc3339)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("query session revoke state: {e}")))?;
        let blacklisted_num: i64 = row.try_get("blacklisted").unwrap_or(0);
        let logout_before: Option<String> = row.try_get("logout_before").unwrap_or(None);
        Ok((blacklisted_num != 0, logout_before))
    }

    pub async fn cleanup_expired_session_records(
        &self,
        now_rfc3339: &str,
    ) -> Result<(u64, u64), AppError> {
        let blacklist_deleted =
            sqlx::query("DELETE FROM session_token_blacklist WHERE expires_at <= ?")
                .bind(now_rfc3339)
                .execute(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("cleanup session blacklist: {e}")))?
                .rows_affected();

        let gate_deleted = sqlx::query("DELETE FROM session_logout_gate WHERE expires_at <= ?")
            .bind(now_rfc3339)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("cleanup session logout gate: {e}")))?
            .rows_affected();

        Ok((blacklist_deleted, gate_deleted))
    }

    pub async fn maybe_cleanup_expired_session_records(
        &self,
        now_utc: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, AppError> {
        let now_ts = now_utc.timestamp();
        let last_ts = LAST_SESSION_CLEANUP_TS.load(Ordering::Relaxed);
        if now_ts - last_ts < SESSION_CLEANUP_INTERVAL_SECS {
            return Ok(false);
        }

        if LAST_SESSION_CLEANUP_TS
            .compare_exchange(last_ts, now_ts, Ordering::SeqCst, Ordering::Relaxed)
            .is_err()
        {
            return Ok(false);
        }

        if let Err(e) = self
            .cleanup_expired_session_records(&now_utc.to_rfc3339())
            .await
        {
            LAST_SESSION_CLEANUP_TS.store(last_ts, Ordering::Relaxed);
            return Err(e);
        }

        Ok(true)
    }
}
