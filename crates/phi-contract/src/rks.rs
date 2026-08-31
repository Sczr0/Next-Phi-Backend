//! RKS 推分契约类型（PushAccHint 从 features/rks/engine.rs 下沉，Phase 1 纯搬迁）。
//! 计算逻辑（target_rks_threshold_from_exact 等）留在 impl-rks。

use serde::{Deserialize, Serialize};

/// 推分 ACC 计算结果（用于区分"无法推分"与"只能推到 100% 才能推分"）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PushAccHint {
    /// 需要将该谱面 ACC 提升到指定值（百分比，保留 3 位小数）才能推分。
    TargetAcc { acc: f64 },
    /// 阈值可达，但只有达到 100.0%（Phi/AP）才能推分。
    PhiOnly,
    /// 即使达到 100.0% 也无法推分。
    Unreachable,
    /// 已满 ACC（>= 100.0%），无需推分。
    AlreadyPhi,
}

impl PushAccHint {
    /// 若该结果可用具体 ACC 表示，则返回目标 ACC（百分比）。
    #[must_use]
    pub const fn target_acc(&self) -> Option<f64> {
        match self {
            Self::TargetAcc { acc } => Some(*acc),
            Self::PhiOnly | Self::Unreachable | Self::AlreadyPhi => None,
        }
    }

    /// 兼容旧逻辑：无法区分时以 100.0 表示"推到顶/无法推分"。
    #[must_use]
    pub const fn as_legacy_acc(&self) -> f64 {
        match self {
            Self::TargetAcc { acc } => *acc,
            Self::PhiOnly | Self::Unreachable | Self::AlreadyPhi => 100.0,
        }
    }
}
