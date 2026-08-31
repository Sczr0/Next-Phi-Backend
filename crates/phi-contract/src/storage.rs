//! 存储端口契约类型（Phase 2：从 impl-storage 的领域行结构体物化）。
//!
//! 这些结构体是「端口（trait）返回值」——契约层持有，实现层（imp-storage）
//! 以 re-export 保持路径不变。纯结构体，零 sqlx、零逻辑。

/// 公开排行榜行（top/seek/by_rank 共用）。
#[derive(Debug, Clone, PartialEq)]
pub struct LeaderboardTopRow {
    pub user_hash: String,
    pub alias: Option<String>,
    pub total_rks: f64,
    pub updated_at: String,
    /// show_best_top3（COALESCE(up.show_best_top3,0)）
    pub sbt: i64,
    /// show_ap_top3（COALESCE(up.show_ap_top3,0)）
    pub sat: i64,
}

/// 公开资料行（user_profile LEFT JOIN leaderboard_rks）。
#[derive(Debug, Clone, PartialEq)]
pub struct PublicProfileRow {
    pub user_hash: String,
    pub is_public: i64,
    pub show_rks_composition: i64,
    pub show_best_top3: i64,
    pub show_ap_top3: i64,
    pub total_rks: f64,
    pub updated_at: String,
}

/// 排行详情行（leaderboard_details，JSON 文本列）。
#[derive(Debug, Clone, PartialEq)]
pub struct LeaderboardDetailsRow {
    pub rks_composition_json: Option<String>,
    pub best_top3_json: Option<String>,
    pub ap_top3_json: Option<String>,
}

/// 用户封禁状态行（user_moderation_state）。
#[derive(Debug, Clone, PartialEq)]
pub struct ModerationStateFullRow {
    pub status: String,
    pub reason: Option<String>,
    pub updated_by: Option<String>,
    pub updated_at: Option<String>,
}

/// 可疑用户行（管理员扫描）。
#[derive(Debug, Clone, PartialEq)]
pub struct SuspiciousRow {
    pub user_hash: String,
    pub alias: Option<String>,
    pub total_rks: f64,
    pub suspicion_score: f64,
    pub updated_at: String,
}

/// 管理员排行榜用户行（含状态筛选）。
#[derive(Debug, Clone, PartialEq)]
pub struct AdminLeaderboardUserRow {
    pub user_hash: String,
    pub alias: Option<String>,
    pub total_rks: f64,
    pub suspicion_score: f64,
    pub is_hidden: i64,
    pub status: String,
    pub updated_at: String,
}
