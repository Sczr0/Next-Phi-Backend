//! 存储端口（trait）与契约测试套件（Charter §5.3——任何实现必须通过）。
//!
//! 设计（r0semi-mp 同款）：`LeaderboardRepo` 是"薄缝"——最小只读查询端口；
//! 契约测试以泛型编写，fake 与真 SQLite 都运行；未来替换实现
//! （如 impl-leaderboard-v2）也必须通过本套件才能上线。

use async_trait::async_trait;

use crate::error::StorageError;
use crate::storage::{
    AdminLeaderboardUserRow, LeaderboardDetailsRow, LeaderboardTopRow, ModerationStateFullRow,
    PublicProfileRow, SuspiciousRow,
};

/// 排行榜存储端口（只读查询面；写入/管理方法随 Phase 2 逐模块扩展）。
///
/// 契约语义（实现层必须一致）：
/// - 公开榜（top/seek/by_rank/count）只含 `is_public=1` 且 `is_hidden=0` 的用户；
/// - 排序恒为 `total_rks DESC, updated_at ASC, user_hash ASC`；
/// - `suspicious_rows` 恒为 `suspicion_score DESC, total_rks DESC, user_hash ASC`；
/// - `admin_users_rows` 全量 rks 用户（含隐藏/封禁），`status='active'` 过滤
///   同时放行 `status IS NULL`；
/// - 公开资料/封禁状态按 alias/user_hash 精确匹配，不存在返回 `None`。
#[async_trait]
pub trait LeaderboardRepo: Send + Sync {
    async fn count_public_total(&self) -> Result<i64, StorageError>;
    async fn top_offset(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<LeaderboardTopRow>, StorageError>;
    async fn top_seek(
        &self,
        after_score: f64,
        after_updated: &str,
        after_user: &str,
        limit: i64,
    ) -> Result<Vec<LeaderboardTopRow>, StorageError>;
    async fn by_rank(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<LeaderboardTopRow>, StorageError>;
    async fn public_profile_by_alias(
        &self,
        alias: &str,
    ) -> Result<Option<PublicProfileRow>, StorageError>;
    async fn details_row(
        &self,
        user_hash: &str,
    ) -> Result<Option<LeaderboardDetailsRow>, StorageError>;
    async fn suspicious_rows(
        &self,
        min_score: f64,
        limit: i64,
    ) -> Result<Vec<SuspiciousRow>, StorageError>;
    async fn admin_users_rows(
        &self,
        status_filter: Option<&str>,
        alias_like: Option<&str>,
        page_size: i64,
        offset: i64,
    ) -> Result<Vec<AdminLeaderboardUserRow>, StorageError>;
    async fn user_moderation_full_row(
        &self,
        user_hash: &str,
    ) -> Result<Option<ModerationStateFullRow>, StorageError>;
}

/// 契约测试套件：任何实现必须通过。
///
/// 前置（seed 契约，各实现测试负责填充）：
/// | user_hash | total_rks | suspicion | is_public | alias    | status   | sbt |
/// |-----------|-----------|-----------|-----------|----------|----------|-----|
/// | apple     | 15.0      | 0.10      | 1         | apples   | (NULL)   | 0   |
/// | orange    | 14.0      | 0.20      | 1         | oranges  | (NULL)   | 1   |
/// | pear      | 13.0      | 0.30      | 1         | pears    | (NULL)   | 0   |
/// | hidden    | 16.0      | 0.80      | 0         | hidden-u | (NULL)   | 0   |
/// | banned    | 12.0      | 0.90      | 0         | banned-u | banned   | 0   |
/// 公开榜 total = 3（apple/orange/pear）。
///
/// # Errors
/// 首个违反契约语义的断言即返回 `Err(String)`（含断言说明，便于定位失败实现）。
#[allow(clippy::too_many_lines)] // 断言密集型契约套件：分段注释即结构，拆分会破坏可读性
pub async fn leaderboard_repo_contract_suite<R: LeaderboardRepo>(repo: &R) -> Result<(), String> {
    // 1. 公开榜计数
    let total = repo.count_public_total().await.map_err(|e| e.to_string())?;
    if total != 3 {
        return Err(format!("count_public_total 期望 3，实际 {total}"));
    }

    // 2. top_offset：limit/排序
    let top = repo.top_offset(2, 0).await.map_err(|e| e.to_string())?;
    if top.len() != 2 {
        return Err(format!("top_offset(2,0) 期望 2 行，实际 {}", top.len()));
    }
    if top[0].user_hash != "apple" || top[1].user_hash != "orange" {
        return Err(format!(
            "top 排序错误（非 RKS DESC）：{} , {}",
            top[0].user_hash, top[1].user_hash
        ));
    }
    if top[0].total_rks < top[1].total_rks {
        return Err("top 未按 total_rks 降序".into());
    }

    // 3. offset 越界：空
    let empty = repo.top_offset(10, 10).await.map_err(|e| e.to_string())?;
    if !empty.is_empty() {
        return Err("top_offset 越界应返回空".into());
    }

    // 4. by_rank 与 top_offset 同语义（同 limit/offset 结果一致）
    let by_rank = repo.by_rank(2, 0).await.map_err(|e| e.to_string())?;
    if by_rank != top {
        return Err("by_rank 语义应与 top_offset 一致".into());
    }

    // 5. seek：以上页末行作游标 -> 严格继续（无重复、顺序正确）
    let Some(last) = top.last() else {
        return Err("top 非空（seed 契约保证 3 条公开记录）".into());
    };
    let seek = repo
        .top_seek(last.total_rks, &last.updated_at, &last.user_hash, 10)
        .await
        .map_err(|e| e.to_string())?;
    if seek.len() != 1 || seek[0].user_hash != "pear" {
        return Err(format!(
            "seek 后继错误：{}",
            seek.iter()
                .map(|r| r.user_hash.as_str())
                .collect::<Vec<_>>()
                .join(",")
        ));
    }

    // 6. 公开资料：存在 / 隐藏
    let apples = repo
        .public_profile_by_alias("apples")
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "apples 公开资料缺失".to_string())?;
    if apples.is_public != 1 || apples.user_hash.is_empty() {
        return Err("apples 资料字段错误".into());
    }
    if let Some(h) = repo
        .public_profile_by_alias("hidden-u")
        .await
        .map_err(|e| e.to_string())?
        && h.is_public != 0
    {
        return Err("隐藏用户应 is_public=0（由读方判定 404）".into());
    }

    // 7. 排行详情
    if repo
        .details_row("apple")
        .await
        .map_err(|e| e.to_string())?
        .is_none()
    {
        return Err("apple 排行详情缺失".into());
    }

    // 8. 可疑用户：阈值过滤 + 降序
    let sus = repo
        .suspicious_rows(0.5, 10)
        .await
        .map_err(|e| e.to_string())?;
    if sus.len() != 2 || sus[0].user_hash != "banned" || sus[1].user_hash != "hidden" {
        return Err(format!(
            "suspicious 过滤/排序错误：{:?}",
            sus.iter().map(|r| r.user_hash.as_str()).collect::<Vec<_>>()
        ));
    }
    if sus.iter().any(|r| r.suspicion_score < 0.5) {
        return Err("suspicious 出现低于阈值的行".into());
    }

    // 9. 管理列表：状态过滤（active 放行 NULL / banned 精确；hidden 属 rks 全量且状态 NULL——计入 active）
    let active = repo
        .admin_users_rows(Some("active"), None, 10, 0)
        .await
        .map_err(|e| e.to_string())?;
    if active.len() != 4 || active.iter().any(|r| r.status != "active") {
        return Err(format!("active 过滤错误：{}", active.len()));
    }
    let banned = repo
        .admin_users_rows(Some("banned"), None, 10, 0)
        .await
        .map_err(|e| e.to_string())?;
    if banned.len() != 1 || banned[0].user_hash != "banned" {
        return Err("banned 过滤错误".into());
    }

    // 10. 封禁状态：存在 / 不存在
    let m = repo
        .user_moderation_full_row("banned")
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "banned 封禁记录缺失".to_string())?;
    if m.status != "banned" {
        return Err("封禁状态字段错误".into());
    }
    if repo
        .user_moderation_full_row("nobody")
        .await
        .map_err(|e| e.to_string())?
        .is_some()
    {
        return Err("不存在用户应返回 None".into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    fn row(user: &str, rks: f64, alias: Option<&str>) -> LeaderboardTopRow {
        LeaderboardTopRow {
            user_hash: user.into(),
            alias: alias.map(str::to_string),
            total_rks: rks,
            updated_at: "2026-01-01T00:00:00Z".into(),
            sbt: 0,
            sat: 0,
        }
    }

    /// 契约排序（实现层必须一致）：rks DESC, updated ASC, user_hash ASC。
    fn top_ordering(a: &LeaderboardTopRow, b: &LeaderboardTopRow) -> std::cmp::Ordering {
        b.total_rks
            .partial_cmp(&a.total_rks)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.updated_at.cmp(&b.updated_at))
            .then(a.user_hash.cmp(&b.user_hash))
    }

    /// 内存 fake（与契约语义严格对齐）：证明套件自洽。
    struct FakeRepo {
        rows: Vec<LeaderboardTopRow>,
        profiles: Vec<PublicProfileRow>,
        details: Vec<LeaderboardDetailsRow>,
        sus: Vec<SuspiciousRow>,
        admin: Vec<AdminLeaderboardUserRow>,
        mods: Vec<ModerationStateFullRow>,
    }

    impl FakeRepo {
        fn seeded() -> Self {
            let mut rows = vec![
                row("apple", 15.0, Some("apples")),
                row("orange", 14.0, Some("oranges")),
                row("pear", 13.0, Some("pears")),
            ];
            rows.sort_by(top_ordering);
            let profiles = vec![
                PublicProfileRow {
                    user_hash: "apple".into(),
                    is_public: 1,
                    show_rks_composition: 0,
                    show_best_top3: 0,
                    show_ap_top3: 0,
                    total_rks: 15.0,
                    updated_at: "2026-01-01T00:00:00Z".into(),
                },
                PublicProfileRow {
                    user_hash: "hidden".into(),
                    is_public: 0,
                    show_rks_composition: 0,
                    show_best_top3: 0,
                    show_ap_top3: 0,
                    total_rks: 16.0,
                    updated_at: "2026-01-01T00:00:00Z".into(),
                },
            ];
            let sus = vec![
                SuspiciousRow {
                    user_hash: "banned".into(),
                    alias: Some("banned-u".into()),
                    total_rks: 12.0,
                    suspicion_score: 0.9,
                    updated_at: "2026-01-01T00:00:00Z".into(),
                },
                SuspiciousRow {
                    user_hash: "hidden".into(),
                    alias: Some("hidden-u".into()),
                    total_rks: 16.0,
                    suspicion_score: 0.8,
                    updated_at: "2026-01-01T00:00:00Z".into(),
                },
            ];
            let admin = vec![
                AdminLeaderboardUserRow {
                    user_hash: "apple".into(),
                    alias: Some("apples".into()),
                    total_rks: 15.0,
                    suspicion_score: 0.1,
                    is_hidden: 0,
                    status: "active".into(),
                    updated_at: "2026-01-01T00:00:00Z".into(),
                },
                AdminLeaderboardUserRow {
                    user_hash: "orange".into(),
                    alias: Some("oranges".into()),
                    total_rks: 14.0,
                    suspicion_score: 0.2,
                    is_hidden: 0,
                    status: "active".into(),
                    updated_at: "2026-01-01T00:00:00Z".into(),
                },
                AdminLeaderboardUserRow {
                    user_hash: "pear".into(),
                    alias: Some("pears".into()),
                    total_rks: 13.0,
                    suspicion_score: 0.3,
                    is_hidden: 0,
                    status: "active".into(),
                    updated_at: "2026-01-01T00:00:00Z".into(),
                },
                AdminLeaderboardUserRow {
                    user_hash: "hidden".into(),
                    alias: Some("hidden-u".into()),
                    total_rks: 16.0,
                    suspicion_score: 0.8,
                    is_hidden: 1,
                    status: "active".into(),
                    updated_at: "2026-01-01T00:00:00Z".into(),
                },
                AdminLeaderboardUserRow {
                    user_hash: "banned".into(),
                    alias: Some("banned-u".into()),
                    total_rks: 12.0,
                    suspicion_score: 0.9,
                    is_hidden: 0,
                    status: "banned".into(),
                    updated_at: "2026-01-01T00:00:00Z".into(),
                },
            ];
            let mods = vec![ModerationStateFullRow {
                status: "banned".into(),
                reason: Some("spam".into()),
                updated_by: Some("admin".into()),
                updated_at: Some("2026-01-01T00:00:00Z".into()),
            }];
            Self {
                rows,
                profiles,
                details: vec![LeaderboardDetailsRow {
                    rks_composition_json: None,
                    best_top3_json: Some("[]".into()),
                    ap_top3_json: Some("[]".into()),
                }],
                sus,
                admin,
                mods,
            }
        }
    }

    #[async_trait]
    impl LeaderboardRepo for FakeRepo {
        async fn count_public_total(&self) -> Result<i64, StorageError> {
            Ok(self.rows.len() as i64)
        }
        async fn top_offset(
            &self,
            limit: i64,
            offset: i64,
        ) -> Result<Vec<LeaderboardTopRow>, StorageError> {
            let start = offset.max(0) as usize;
            Ok(self
                .rows
                .iter()
                .skip(start)
                .take(limit.max(0) as usize)
                .cloned()
                .collect())
        }
        async fn top_seek(
            &self,
            after_score: f64,
            after_updated: &str,
            after_user: &str,
            limit: i64,
        ) -> Result<Vec<LeaderboardTopRow>, StorageError> {
            let cursor = LeaderboardTopRow {
                user_hash: after_user.into(),
                alias: None,
                total_rks: after_score,
                updated_at: after_updated.into(),
                sbt: 0,
                sat: 0,
            };
            let out: Vec<_> = self
                .rows
                .iter()
                .filter(|r| top_ordering(r, &cursor) == std::cmp::Ordering::Greater)
                .take(limit.max(0) as usize)
                .cloned()
                .collect();
            Ok(out)
        }
        async fn by_rank(
            &self,
            limit: i64,
            offset: i64,
        ) -> Result<Vec<LeaderboardTopRow>, StorageError> {
            self.top_offset(limit, offset).await
        }
        async fn public_profile_by_alias(
            &self,
            alias: &str,
        ) -> Result<Option<PublicProfileRow>, StorageError> {
            // seed 中 alias != user_hash：做双向映射。
            let user_hash = match alias {
                "apples" => "apple",
                "hidden-u" => "hidden",
                _ => alias,
            };
            Ok(self
                .profiles
                .iter()
                .find(|p| p.user_hash == user_hash)
                .cloned())
        }
        async fn details_row(
            &self,
            user_hash: &str,
        ) -> Result<Option<LeaderboardDetailsRow>, StorageError> {
            Ok(if user_hash == "apple" {
                self.details.first().cloned()
            } else {
                None
            })
        }
        async fn suspicious_rows(
            &self,
            min_score: f64,
            limit: i64,
        ) -> Result<Vec<SuspiciousRow>, StorageError> {
            let mut v: Vec<_> = self
                .sus
                .iter()
                .filter(|r| r.suspicion_score >= min_score)
                .cloned()
                .collect();
            v.sort_by(|a, b| {
                b.suspicion_score
                    .partial_cmp(&a.suspicion_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            Ok(v.into_iter().take(limit.max(0) as usize).collect())
        }
        async fn admin_users_rows(
            &self,
            status_filter: Option<&str>,
            _alias_like: Option<&str>,
            page_size: i64,
            offset: i64,
        ) -> Result<Vec<AdminLeaderboardUserRow>, StorageError> {
            let mut v: Vec<_> = self.admin.iter().cloned().collect();
            if let Some(s) = status_filter {
                v.retain(|r| {
                    if s.eq_ignore_ascii_case("active") {
                        r.status == "active"
                    } else {
                        r.status.eq_ignore_ascii_case(s)
                    }
                });
            }
            let start = offset.max(0) as usize;
            Ok(v.into_iter()
                .skip(start)
                .take(page_size.max(0) as usize)
                .collect())
        }
        async fn user_moderation_full_row(
            &self,
            user_hash: &str,
        ) -> Result<Option<ModerationStateFullRow>, StorageError> {
            Ok(if user_hash == "banned" {
                self.mods.first().cloned()
            } else {
                None
            })
        }
    }

    #[tokio::test]
    async fn fake_repo_passes_contract_suite() {
        let repo = FakeRepo::seeded();
        leaderboard_repo_contract_suite(&repo)
            .await
            .expect("fake 应通过契约套件");
    }
}
