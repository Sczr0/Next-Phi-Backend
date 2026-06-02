use sqlx::{QueryBuilder, Row, Sqlite, sqlite::SqliteRow};

use crate::error::AppError;

use super::{RksHistoryEntry, StatsStorage, SubmissionRecord, UserAliasDefaults};

impl StatsStorage {
    pub async fn get_prev_rks(&self, user_hash: &str) -> Result<Option<(f64, String)>, AppError> {
        let row =
            sqlx::query("SELECT total_rks, updated_at FROM leaderboard_rks WHERE user_hash = ?")
                .bind(user_hash)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("get prev rks: {e}")))?;
        if let Some(r) = row {
            Ok(Some((
                r.get::<f64, _>("total_rks"),
                r.get::<String, _>("updated_at"),
            )))
        } else {
            Ok(None)
        }
    }

    pub async fn count_public_leaderboard_total(&self) -> Result<i64, AppError> {
        let row = sqlx::query("SELECT COUNT(1) AS c FROM leaderboard_rks lr LEFT JOIN user_profile up ON up.user_hash=lr.user_hash WHERE COALESCE(up.is_public,0)=1 AND lr.is_hidden=0")
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
        sqlx::query(
            "SELECT lr.user_hash, lr.total_rks, lr.updated_at, up.alias, COALESCE(up.show_best_top3,0) AS sbt, COALESCE(up.show_ap_top3,0) AS sat
             FROM leaderboard_rks lr LEFT JOIN user_profile up ON up.user_hash=lr.user_hash
             WHERE COALESCE(up.is_public,0)=1 AND lr.is_hidden=0 AND (
               lr.total_rks < ? OR (lr.total_rks = ? AND (lr.updated_at > ? OR (lr.updated_at = ? AND lr.user_hash > ?)))
             )
             ORDER BY lr.total_rks DESC, lr.updated_at ASC, lr.user_hash ASC
             LIMIT ?",
        )
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
        sqlx::query(
            "SELECT lr.user_hash, lr.total_rks, lr.updated_at, up.alias, COALESCE(up.show_best_top3,0) AS sbt, COALESCE(up.show_ap_top3,0) AS sat
             FROM leaderboard_rks lr LEFT JOIN user_profile up ON up.user_hash=lr.user_hash
             WHERE COALESCE(up.is_public,0)=1 AND lr.is_hidden=0
             ORDER BY lr.total_rks DESC, lr.updated_at ASC, lr.user_hash ASC
             LIMIT ? OFFSET ?",
        )
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
        let row = sqlx::query(
            "SELECT COUNT(1) as higher FROM leaderboard_rks lr LEFT JOIN user_profile up ON up.user_hash=lr.user_hash
             WHERE COALESCE(up.is_public,0)=1 AND lr.is_hidden=0 AND (
               lr.total_rks > ? OR (lr.total_rks = ? AND (lr.updated_at < ? OR (lr.updated_at = ? AND lr.user_hash < ?)))
             )",
        )
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

    pub async fn query_suspicious_rows(
        &self,
        min_score: f64,
        limit: i64,
    ) -> Result<Vec<SqliteRow>, AppError> {
        sqlx::query(
            "SELECT lr.user_hash, lr.total_rks, lr.suspicion_score, lr.updated_at, up.alias
             FROM leaderboard_rks lr LEFT JOIN user_profile up ON up.user_hash=lr.user_hash
             WHERE lr.suspicion_score >= ?
             ORDER BY lr.suspicion_score DESC, lr.total_rks DESC
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
        let mut count_qb = QueryBuilder::<Sqlite>::new(
            "SELECT COUNT(1) AS c
             FROM leaderboard_rks lr
             LEFT JOIN user_profile up ON up.user_hash=lr.user_hash
             LEFT JOIN user_moderation_state ums ON ums.user_hash=lr.user_hash
             WHERE 1=1",
        );
        if let Some(status) = status_filter {
            count_qb
                .push(" AND LOWER(COALESCE(ums.status,'active')) = ")
                .push_bind(status.to_string());
        }
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
            qb.push(" AND LOWER(COALESCE(ums.status,'active')) = ")
                .push_bind(status.to_string());
        }
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

    pub async fn upsert_leaderboard_rks(
        &self,
        user_hash: &str,
        total_rks: f64,
        user_kind: Option<&str>,
        suspicion_score: f64,
        hide: bool,
        now_rfc3339: &str,
    ) -> Result<(), AppError> {
        let is_hidden_i = i64::from(hide);
        sqlx::query(
            "INSERT INTO leaderboard_rks(user_hash,total_rks,user_kind,suspicion_score,is_hidden,created_at,updated_at) VALUES(?,?,?,?,?,?,?)
             ON CONFLICT(user_hash) DO UPDATE SET
               total_rks = CASE WHEN excluded.total_rks > leaderboard_rks.total_rks THEN excluded.total_rks ELSE leaderboard_rks.total_rks END,
               updated_at = CASE WHEN excluded.total_rks > leaderboard_rks.total_rks THEN excluded.updated_at ELSE leaderboard_rks.updated_at END,
               user_kind = COALESCE(excluded.user_kind, leaderboard_rks.user_kind),
               suspicion_score = excluded.suspicion_score,
               is_hidden = CASE WHEN leaderboard_rks.is_hidden=1 OR excluded.is_hidden=1 THEN 1 ELSE 0 END"
        )
        .bind(user_hash)
        .bind(total_rks)
        .bind(user_kind)
        .bind(suspicion_score)
        .bind(is_hidden_i)
        .bind(now_rfc3339)
        .bind(now_rfc3339)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("upsert leaderboard: {e}")))?;
        Ok(())
    }

    pub async fn set_leaderboard_hidden(
        &self,
        user_hash: &str,
        hide: bool,
    ) -> Result<(), AppError> {
        let is_hidden_i = i64::from(hide);
        sqlx::query("UPDATE leaderboard_rks SET is_hidden=? WHERE user_hash=?")
            .bind(is_hidden_i)
            .bind(user_hash)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("update leaderboard hidden: {e}")))?;
        Ok(())
    }

    pub async fn upsert_details(
        &self,
        user_hash: &str,
        rks_comp_json: Option<&str>,
        best3_json: Option<&str>,
        ap3_json: Option<&str>,
        now_rfc3339: &str,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO leaderboard_details(user_hash,rks_composition_json,best_top3_json,ap_top3_json,updated_at) VALUES(?,?,?,?,?)
             ON CONFLICT(user_hash) DO UPDATE SET
               rks_composition_json = COALESCE(excluded.rks_composition_json, leaderboard_details.rks_composition_json),
               best_top3_json = COALESCE(excluded.best_top3_json, leaderboard_details.best_top3_json),
               ap_top3_json = COALESCE(excluded.ap_top3_json, leaderboard_details.ap_top3_json),
               updated_at = excluded.updated_at"
        )
        .bind(user_hash)
        .bind(rks_comp_json)
        .bind(best3_json)
        .bind(ap3_json)
        .bind(now_rfc3339)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("upsert details: {e}")))?;
        Ok(())
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

    /// 查询用户 RKS 历史记录
    ///
    /// 返回 (历史记录列表, 总记录数)
    pub async fn query_rks_history(
        &self,
        user_hash: &str,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<RksHistoryEntry>, i64), AppError> {
        // 归一化浮点噪声：避免把 1e-15 量级差值当成“RKS 变化”暴露给客户端。
        const RKS_JUMP_EPS: f64 = 1e-9;

        // 查询总数
        let count_row =
            sqlx::query("SELECT COUNT(1) as c FROM save_submissions WHERE user_hash = ?")
                .bind(user_hash)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("count rks history: {e}")))?;
        let total: i64 = count_row.try_get("c").unwrap_or(0);

        // 查询历史记录（按时间倒序）
        let rows = sqlx::query(
            "SELECT total_rks, rks_jump, created_at FROM save_submissions WHERE user_hash = ? ORDER BY created_at DESC LIMIT ? OFFSET ?"
        )
            .bind(user_hash)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("query rks history: {e}")))?;

        let items: Vec<RksHistoryEntry> = rows
            .into_iter()
            .map(|r| {
                let rks = r.try_get::<f64, _>("total_rks").unwrap_or(0.0);
                let rks_jump = r.try_get::<f64, _>("rks_jump").unwrap_or(0.0);
                let rks_jump = if rks_jump.abs() < RKS_JUMP_EPS {
                    0.0
                } else {
                    rks_jump
                };
                RksHistoryEntry {
                    rks,
                    rks_jump,
                    created_at: r.try_get::<String, _>("created_at").unwrap_or_default(),
                }
            })
            .collect();

        Ok((items, total))
    }

    /// 获取用户历史最高 RKS
    pub async fn get_peak_rks(&self, user_hash: &str) -> Result<f64, AppError> {
        let row =
            sqlx::query("SELECT MAX(total_rks) as peak FROM save_submissions WHERE user_hash = ?")
                .bind(user_hash)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("get peak rks: {e}")))?;

        Ok(row.try_get::<f64, _>("peak").unwrap_or(0.0))
    }
}
