//! 委托授权存储：authorization request、code、access/refresh token、委托记录。
//!
//! sqlx 只允许出现在本 crate；SQL 常量集中在这里，业务层只拿结构化行。

use std::path::Path;
use std::time::Duration;

use sqlx::{
    ConnectOptions, Row,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use sqlx::{SqlitePool, sqlite::SqliteJournalMode, sqlite::SqliteSynchronous};

use crate::error::AppError;

fn db(context: &str, error: &sqlx::Error) -> AppError {
    AppError::Internal(format!("{context}: {error}"))
}

#[derive(Clone)]
pub struct DelegationStorage {
    pub pool: SqlitePool,
}

#[derive(Debug, Clone)]
pub struct RequestRow {
    pub id: String,
    pub client_id: String,
    pub redirect_uri: String,
    pub state: Option<String>,
    pub code_challenge: String,
    pub scope: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone)]
pub struct DelegationRow {
    pub id: String,
    pub client_id: String,
    pub subject: String,
    pub scopes: String,
    pub credential_ciphertext: String,
    pub credential_key_version: i64,
    pub expires_at: i64,
    pub revoked_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct CodeRow {
    pub code_hash: String,
    pub delegation_id: String,
    pub client_id: String,
    pub redirect_uri: String,
    pub code_challenge: String,
    pub scopes: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone)]
pub struct AccessContext {
    pub token_hash: String,
    pub delegation_id: String,
    pub subject: String,
    pub scopes: String,
    pub credential_ciphertext: String,
    pub credential_key_version: i64,
    pub expires_at: i64,
    pub revoked_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct RefreshRow {
    pub token_hash: String,
    pub family_id: String,
    pub delegation_id: String,
    pub expires_at: i64,
    pub consumed_at: Option<i64>,
    pub revoked_at: Option<i64>,
}

impl DelegationStorage {
    pub async fn connect_sqlite(path: &str, wal: bool) -> Result<Self, AppError> {
        let mut opt = SqliteConnectOptions::new()
            .filename(Path::new(path))
            .create_if_missing(true)
            .busy_timeout(Duration::from_secs(5))
            .foreign_keys(true)
            .synchronous(SqliteSynchronous::Normal)
            .log_statements(tracing::log::LevelFilter::Off);
        if wal {
            opt = opt.journal_mode(SqliteJournalMode::Wal);
        }
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .acquire_timeout(Duration::from_secs(10))
            .connect_with(opt)
            .await
            .map_err(|e| AppError::Internal(format!("delegation sqlite connect: {e}")))?;
        Ok(Self { pool })
    }

    pub async fn init_schema(&self) -> Result<(), AppError> {
        let ddl = r"
        CREATE TABLE IF NOT EXISTS delegation_requests (
          id TEXT PRIMARY KEY,
          client_id TEXT NOT NULL,
          redirect_uri TEXT NOT NULL,
          state TEXT,
          code_challenge TEXT NOT NULL,
          scope TEXT NOT NULL,
          created_at INTEGER NOT NULL,
          expires_at INTEGER NOT NULL,
          consumed_at INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_delegation_requests_expires
          ON delegation_requests(expires_at);

        CREATE TABLE IF NOT EXISTS delegations (
          id TEXT PRIMARY KEY,
          client_id TEXT NOT NULL,
          subject TEXT NOT NULL,
          scopes TEXT NOT NULL,
          credential_ciphertext TEXT NOT NULL,
          credential_key_version INTEGER NOT NULL,
          created_at INTEGER NOT NULL,
          expires_at INTEGER NOT NULL,
          revoked_at INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_delegations_subject_client
          ON delegations(subject, client_id);
        CREATE INDEX IF NOT EXISTS idx_delegations_expires
          ON delegations(expires_at);

        CREATE TABLE IF NOT EXISTS delegation_codes (
          code_hash TEXT PRIMARY KEY,
          delegation_id TEXT NOT NULL,
          client_id TEXT NOT NULL,
          redirect_uri TEXT NOT NULL,
          code_challenge TEXT NOT NULL,
          scopes TEXT NOT NULL,
          created_at INTEGER NOT NULL,
          expires_at INTEGER NOT NULL,
          consumed_at INTEGER,
          FOREIGN KEY (delegation_id) REFERENCES delegations(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_delegation_codes_expires
          ON delegation_codes(expires_at);

        CREATE TABLE IF NOT EXISTS delegation_access_tokens (
          token_hash TEXT PRIMARY KEY,
          delegation_id TEXT NOT NULL,
          scopes TEXT NOT NULL,
          created_at INTEGER NOT NULL,
          expires_at INTEGER NOT NULL,
          revoked_at INTEGER,
          FOREIGN KEY (delegation_id) REFERENCES delegations(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_delegation_access_delegation
          ON delegation_access_tokens(delegation_id);
        CREATE INDEX IF NOT EXISTS idx_delegation_access_expires
          ON delegation_access_tokens(expires_at);

        CREATE TABLE IF NOT EXISTS delegation_refresh_tokens (
          token_hash TEXT PRIMARY KEY,
          family_id TEXT NOT NULL,
          delegation_id TEXT NOT NULL,
          created_at INTEGER NOT NULL,
          expires_at INTEGER NOT NULL,
          consumed_at INTEGER,
          revoked_at INTEGER,
          FOREIGN KEY (delegation_id) REFERENCES delegations(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_delegation_refresh_family
          ON delegation_refresh_tokens(family_id);
        CREATE INDEX IF NOT EXISTS idx_delegation_refresh_delegation
          ON delegation_refresh_tokens(delegation_id);
        CREATE INDEX IF NOT EXISTS idx_delegation_refresh_expires
          ON delegation_refresh_tokens(expires_at);
        ";
        sqlx::query(ddl)
            .execute(&self.pool)
            .await
            .map_err(|e| db("delegation init schema", &e))?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_request(
        &self,
        id: &str,
        client_id: &str,
        redirect_uri: &str,
        state: Option<&str>,
        code_challenge: &str,
        scope: &str,
        now: i64,
        expires_at: i64,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO delegation_requests
             (id, client_id, redirect_uri, state, code_challenge, scope, created_at, expires_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(client_id)
        .bind(redirect_uri)
        .bind(state)
        .bind(code_challenge)
        .bind(scope)
        .bind(now)
        .bind(expires_at)
        .execute(&self.pool)
        .await
        .map_err(|e| db("delegation insert request", &e))?;
        Ok(())
    }

    pub async fn request(&self, id: &str) -> Result<Option<RequestRow>, AppError> {
        let row = sqlx::query(
            "SELECT id, client_id, redirect_uri, state, code_challenge, scope, expires_at
             FROM delegation_requests WHERE id = ? AND consumed_at IS NULL",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| db("delegation get request", &e))?;
        Ok(row.map(|row| RequestRow {
            id: row.get("id"),
            client_id: row.get("client_id"),
            redirect_uri: row.get("redirect_uri"),
            state: row.get("state"),
            code_challenge: row.get("code_challenge"),
            scope: row.get("scope"),
            expires_at: row.get("expires_at"),
        }))
    }

    pub async fn consume_request(&self, id: &str, now: i64) -> Result<bool, AppError> {
        let result = sqlx::query(
            "UPDATE delegation_requests SET consumed_at = ?
             WHERE id = ? AND consumed_at IS NULL AND expires_at > ?",
        )
        .bind(now)
        .bind(id)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| db("delegation consume request", &e))?;
        Ok(result.rows_affected() == 1)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_delegation(
        &self,
        id: &str,
        client_id: &str,
        subject: &str,
        scopes: &str,
        credential_ciphertext: &str,
        credential_key_version: i64,
        now: i64,
        expires_at: i64,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO delegations
             (id, client_id, subject, scopes, credential_ciphertext, credential_key_version, created_at, expires_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(client_id)
        .bind(subject)
        .bind(scopes)
        .bind(credential_ciphertext)
        .bind(credential_key_version)
        .bind(now)
        .bind(expires_at)
        .execute(&self.pool)
        .await
        .map_err(|e| db("delegation insert delegation", &e))?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_code(
        &self,
        code_hash: &str,
        delegation_id: &str,
        client_id: &str,
        redirect_uri: &str,
        code_challenge: &str,
        scopes: &str,
        now: i64,
        expires_at: i64,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO delegation_codes
             (code_hash, delegation_id, client_id, redirect_uri, code_challenge, scopes, created_at, expires_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(code_hash)
        .bind(delegation_id)
        .bind(client_id)
        .bind(redirect_uri)
        .bind(code_challenge)
        .bind(scopes)
        .bind(now)
        .bind(expires_at)
        .execute(&self.pool)
        .await
        .map_err(|e| db("delegation insert code", &e))?;
        Ok(())
    }

    pub async fn consume_code(
        &self,
        code_hash: &str,
        now: i64,
    ) -> Result<Option<CodeRow>, AppError> {
        let result = sqlx::query(
            "UPDATE delegation_codes SET consumed_at = ?
             WHERE code_hash = ? AND consumed_at IS NULL AND expires_at > ?",
        )
        .bind(now)
        .bind(code_hash)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| db("delegation consume code", &e))?;
        if result.rows_affected() != 1 {
            return Ok(None);
        }
        let row = sqlx::query(
            "SELECT code_hash, delegation_id, client_id, redirect_uri, code_challenge, scopes, expires_at
             FROM delegation_codes WHERE code_hash = ?",
        )
        .bind(code_hash)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| db("delegation read code", &e))?;
        Ok(Some(CodeRow {
            code_hash: row.get("code_hash"),
            delegation_id: row.get("delegation_id"),
            client_id: row.get("client_id"),
            redirect_uri: row.get("redirect_uri"),
            code_challenge: row.get("code_challenge"),
            scopes: row.get("scopes"),
            expires_at: row.get("expires_at"),
        }))
    }

    pub async fn delegation(&self, id: &str) -> Result<Option<DelegationRow>, AppError> {
        let row = sqlx::query(
            "SELECT id, client_id, subject, scopes, credential_ciphertext,
                    credential_key_version, expires_at, revoked_at
             FROM delegations WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| db("delegation get delegation", &e))?;
        Ok(row.map(|row| DelegationRow {
            id: row.get("id"),
            client_id: row.get("client_id"),
            subject: row.get("subject"),
            scopes: row.get("scopes"),
            credential_ciphertext: row.get("credential_ciphertext"),
            credential_key_version: row.get("credential_key_version"),
            expires_at: row.get("expires_at"),
            revoked_at: row.get("revoked_at"),
        }))
    }

    pub async fn insert_access(
        &self,
        token_hash: &str,
        delegation_id: &str,
        scopes: &str,
        now: i64,
        expires_at: i64,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO delegation_access_tokens
             (token_hash, delegation_id, scopes, created_at, expires_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(token_hash)
        .bind(delegation_id)
        .bind(scopes)
        .bind(now)
        .bind(expires_at)
        .execute(&self.pool)
        .await
        .map_err(|e| db("delegation insert access", &e))?;
        Ok(())
    }

    pub async fn authenticate_access(
        &self,
        token_hash: &str,
        now: i64,
    ) -> Result<Option<AccessContext>, AppError> {
        let row = sqlx::query(
            "SELECT t.token_hash, t.delegation_id, t.scopes, t.expires_at, t.revoked_at,
                    d.subject, d.credential_ciphertext, d.credential_key_version
             FROM delegation_access_tokens t
             JOIN delegations d ON d.id = t.delegation_id
             WHERE t.token_hash = ? AND t.expires_at > ? AND t.revoked_at IS NULL
               AND d.expires_at > ? AND d.revoked_at IS NULL",
        )
        .bind(token_hash)
        .bind(now)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| db("delegation authenticate access", &e))?;
        Ok(row.map(|row| AccessContext {
            token_hash: row.get("token_hash"),
            delegation_id: row.get("delegation_id"),
            subject: row.get("subject"),
            scopes: row.get("scopes"),
            credential_ciphertext: row.get("credential_ciphertext"),
            credential_key_version: row.get("credential_key_version"),
            expires_at: row.get("expires_at"),
            revoked_at: row.get("revoked_at"),
        }))
    }

    pub async fn insert_refresh(
        &self,
        token_hash: &str,
        family_id: &str,
        delegation_id: &str,
        now: i64,
        expires_at: i64,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO delegation_refresh_tokens
             (token_hash, family_id, delegation_id, created_at, expires_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(token_hash)
        .bind(family_id)
        .bind(delegation_id)
        .bind(now)
        .bind(expires_at)
        .execute(&self.pool)
        .await
        .map_err(|e| db("delegation insert refresh", &e))?;
        Ok(())
    }

    pub async fn refresh(&self, token_hash: &str) -> Result<Option<RefreshRow>, AppError> {
        let row = sqlx::query(
            "SELECT token_hash, family_id, delegation_id, expires_at, consumed_at, revoked_at
             FROM delegation_refresh_tokens WHERE token_hash = ?",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| db("delegation get refresh", &e))?;
        Ok(row.map(|row| RefreshRow {
            token_hash: row.get("token_hash"),
            family_id: row.get("family_id"),
            delegation_id: row.get("delegation_id"),
            expires_at: row.get("expires_at"),
            consumed_at: row.get("consumed_at"),
            revoked_at: row.get("revoked_at"),
        }))
    }

    pub async fn consume_refresh(
        &self,
        token_hash: &str,
        now: i64,
    ) -> Result<Option<RefreshRow>, AppError> {
        let result = sqlx::query(
            "UPDATE delegation_refresh_tokens SET consumed_at = ?
             WHERE token_hash = ? AND consumed_at IS NULL AND revoked_at IS NULL AND expires_at > ?",
        )
        .bind(now)
        .bind(token_hash)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| db("delegation consume refresh", &e))?;
        if result.rows_affected() != 1 {
            return Ok(None);
        }
        let row = sqlx::query(
            "SELECT token_hash, family_id, delegation_id, expires_at, consumed_at, revoked_at
             FROM delegation_refresh_tokens WHERE token_hash = ?",
        )
        .bind(token_hash)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| db("delegation read refresh", &e))?;
        Ok(Some(RefreshRow {
            token_hash: row.get("token_hash"),
            family_id: row.get("family_id"),
            delegation_id: row.get("delegation_id"),
            expires_at: row.get("expires_at"),
            consumed_at: row.get("consumed_at"),
            revoked_at: row.get("revoked_at"),
        }))
    }

    pub async fn revoke_family(&self, family_id: &str, now: i64) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE delegation_refresh_tokens SET revoked_at = ?
             WHERE family_id = ? AND revoked_at IS NULL",
        )
        .bind(now)
        .bind(family_id)
        .execute(&self.pool)
        .await
        .map_err(|e| db("delegation revoke family", &e))?;
        Ok(())
    }

    pub async fn revoke_delegation(&self, delegation_id: &str, now: i64) -> Result<(), AppError> {
        sqlx::query("UPDATE delegations SET revoked_at = ? WHERE id = ? AND revoked_at IS NULL")
            .bind(now)
            .bind(delegation_id)
            .execute(&self.pool)
            .await
            .map_err(|e| db("delegation revoke delegation", &e))?;
        sqlx::query(
            "UPDATE delegation_access_tokens SET revoked_at = ?
             WHERE delegation_id = ? AND revoked_at IS NULL",
        )
        .bind(now)
        .bind(delegation_id)
        .execute(&self.pool)
        .await
        .map_err(|e| db("delegation revoke access", &e))?;
        sqlx::query(
            "UPDATE delegation_refresh_tokens SET revoked_at = ?
             WHERE delegation_id = ? AND revoked_at IS NULL",
        )
        .bind(now)
        .bind(delegation_id)
        .execute(&self.pool)
        .await
        .map_err(|e| db("delegation revoke refresh", &e))?;
        Ok(())
    }

    pub async fn revoke_access(&self, token_hash: &str, now: i64) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE delegation_access_tokens SET revoked_at = ?
             WHERE token_hash = ? AND revoked_at IS NULL",
        )
        .bind(now)
        .bind(token_hash)
        .execute(&self.pool)
        .await
        .map_err(|e| db("delegation revoke access token", &e))?;
        Ok(())
    }

    pub async fn find_refresh_delegation(
        &self,
        token_hash: &str,
    ) -> Result<Option<String>, AppError> {
        let row = sqlx::query(
            "SELECT delegation_id FROM delegation_refresh_tokens WHERE token_hash = ? LIMIT 1",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| db("delegation find refresh", &e))?;
        Ok(row.map(|row| row.get("delegation_id")))
    }

    pub async fn cleanup(&self, now: i64) -> Result<(), AppError> {
        for sql in [
            "DELETE FROM delegation_requests WHERE expires_at <= ?",
            "DELETE FROM delegation_codes WHERE expires_at <= ?",
            "DELETE FROM delegation_access_tokens WHERE expires_at <= ?",
            "DELETE FROM delegation_refresh_tokens WHERE expires_at <= ?",
            "DELETE FROM delegations WHERE expires_at <= ?",
        ] {
            sqlx::query(sql)
                .bind(now)
                .execute(&self.pool)
                .await
                .map_err(|e| db("delegation cleanup", &e))?;
        }
        Ok(())
    }
}
