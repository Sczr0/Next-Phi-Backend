use std::collections::HashMap;

use crate::features::save::models::{Difficulty, DifficultyRecord};
use crate::startup::chart_loader::{ChartConstants, ChartConstantsMap};
use serde::{Deserialize, Serialize};

/// 单张谱面的 RKS 结果
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChartRankingScore {
    /// 歌曲 ID
    #[schema(example = "97f9466b2e77")]
    pub song_id: String,
    pub difficulty: Difficulty,
    /// 谱面 RKS 值
    #[schema(example = 12.34)]
    pub rks: f64,
}

/// 计算单张谱面的 RKS
///
/// 参数 `accuracy` 采用小数形式（例如 98.5% -> 0.985）。
/// 当 `accuracy` < 0.70 时，直接返回 0.0。
pub fn calculate_single_chart_rks(accuracy: f32, chart_constant: f32) -> f64 {
    let acc = accuracy as f64;
    if acc < 0.70 {
        return 0.0;
    }
    let level = chart_constant as f64;
    let ratio = ((100.0 * acc) - 55.0) / 45.0;
    let score = (ratio * ratio) * level;
    if score.is_finite() && score > 0.0 {
        score
    } else {
        0.0
    }
}

/// 玩家 RKS 计算结果
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlayerRksResult {
    /// 玩家总 RKS （Best27 + AP3）/ 30
    #[schema(example = 14.56)]
    pub total_rks: f64,
    pub b30_charts: Vec<ChartRankingScore>,
}

/// 根据玩家成绩与定数表计算 B30 与总 RKS
#[deprecated(note = "Use calculate_player_rks_simplified (简化法)")]
pub fn calculate_player_rks(
    records: &HashMap<String, Vec<DifficultyRecord>>,
    chart_constants: &ChartConstantsMap,
) -> PlayerRksResult {
    // 收集所有有效的谱面 RKS
    let mut all_scores: Vec<ChartRankingScore> = Vec::new();
    let mut phi_scores: Vec<ChartRankingScore> = Vec::new();

    for (song_id, diffs) in records.iter() {
        for rec in diffs {
            // 定数查找
            let Some(consts) = chart_constants.get(song_id) else {
                continue;
            };
            let Some(level) = level_for_difficulty(consts, &rec.difficulty) else {
                continue;
            };

            // 统一单位：记录里常见为百分比（如 98.5），公式需要小数
            let acc_percent = rec.accuracy as f64;
            let acc_decimal = if acc_percent > 1.5 {
                acc_percent / 100.0
            } else {
                acc_percent
            } as f32;

            let rks_value = calculate_single_chart_rks(acc_decimal, level);
            let entry = ChartRankingScore {
                song_id: song_id.clone(),
                difficulty: rec.difficulty.clone(),
                rks: rks_value,
            };

            all_scores.push(entry.clone());
            // φ 评级：accuracy >= 100.0（百分比语义）
            if acc_percent >= 100.0 {
                phi_scores.push(entry);
            }
        }
    }

    // 按 rks 值降序排序
    all_scores.sort_by(|a, b| {
        b.rks
            .partial_cmp(&a.rks)
            .unwrap_or(core::cmp::Ordering::Equal)
    });
    phi_scores.sort_by(|a, b| {
        b.rks
            .partial_cmp(&a.rks)
            .unwrap_or(core::cmp::Ordering::Equal)
    });

    const TOP_GENERAL: usize = 27;
    const TOP_PHI: usize = 3;

    // 取前 27 个总体成绩
    let mut picked: Vec<ChartRankingScore> = all_scores.iter().take(TOP_GENERAL).cloned().collect();

    // 从 φ 列表取前 3 个，避免与已选重复
    let mut picked_keys = picked
        .iter()
        .map(|c| (c.song_id.clone(), key_of_difficulty(&c.difficulty)))
        .collect::<std::collections::HashSet<_>>();

    for cs in phi_scores.iter() {
        if picked.len() >= TOP_GENERAL + TOP_PHI {
            break;
        }
        let key = (cs.song_id.clone(), key_of_difficulty(&cs.difficulty));
        if !picked_keys.contains(&key) {
            picked.push(cs.clone());
            picked_keys.insert(key);
        }
    }

    // 计算总 RKS：不足 30 个按 30 作为分母（缺口视为 0）
    let sum: f64 = picked.iter().map(|c| c.rks).sum();
    let total_rks = sum / 30.0;

    PlayerRksResult {
        total_rks,
        b30_charts: picked,
    }
}

/// 根据玩家成绩与定数表计算 B30 与总 RKS（简化法：Best27 + AP3，允许重叠）
pub fn calculate_player_rks_simplified(
    records: &HashMap<String, Vec<DifficultyRecord>>,
    chart_constants: &ChartConstantsMap,
) -> PlayerRksResult {
    // 收集所有有效谱面 RKS 与 AP 列表
    let mut all_scores: Vec<ChartRankingScore> = Vec::new();
    let mut phi_scores: Vec<ChartRankingScore> = Vec::new();

    for (song_id, diffs) in records.iter() {
        for rec in diffs {
            let Some(consts) = chart_constants.get(song_id) else {
                continue;
            };
            let Some(level) = level_for_difficulty(consts, &rec.difficulty) else {
                continue;
            };

            let acc_percent = rec.accuracy as f64;
            let acc_decimal = if acc_percent > 1.5 {
                acc_percent / 100.0
            } else {
                acc_percent
            } as f32;
            let rks_value = calculate_single_chart_rks(acc_decimal, level);
            let entry = ChartRankingScore {
                song_id: song_id.clone(),
                difficulty: rec.difficulty.clone(),
                rks: rks_value,
            };
            all_scores.push(entry.clone());
            if acc_percent >= 100.0 {
                phi_scores.push(entry);
            }
        }
    }

    all_scores.sort_by(|a, b| {
        b.rks
            .partial_cmp(&a.rks)
            .unwrap_or(core::cmp::Ordering::Equal)
    });
    phi_scores.sort_by(|a, b| {
        b.rks
            .partial_cmp(&a.rks)
            .unwrap_or(core::cmp::Ordering::Equal)
    });

    const TOP_GENERAL: usize = 27;
    const TOP_PHI: usize = 3;

    // 简化法组成：Top27 + AP Top3（允许重叠）
    let mut picked: Vec<ChartRankingScore> = all_scores.iter().take(TOP_GENERAL).cloned().collect();
    let ap_top3: Vec<ChartRankingScore> = phi_scores.iter().take(TOP_PHI).cloned().collect();
    picked.extend(ap_top3.iter().cloned());

    let sum_best27: f64 = all_scores.iter().take(TOP_GENERAL).map(|c| c.rks).sum();
    let sum_ap3: f64 = phi_scores.iter().take(TOP_PHI).map(|c| c.rks).sum();
    let total_rks = (sum_best27 + sum_ap3) / 30.0;

    PlayerRksResult {
        total_rks,
        b30_charts: picked,
    }
}

fn level_for_difficulty(consts: &ChartConstants, diff: &Difficulty) -> Option<f32> {
    match diff {
        Difficulty::EZ => consts.ez,
        Difficulty::HD => consts.hd,
        Difficulty::IN => consts.in_level,
        Difficulty::AT => consts.at,
    }
}

fn key_of_difficulty(diff: &Difficulty) -> u8 {
    match diff {
        Difficulty::EZ => 0,
        Difficulty::HD => 1,
        Difficulty::IN => 2,
        Difficulty::AT => 3,
    }
}

// --- 兼容旧实现的 RKS 详情与推分算法 ---

/// 统一的 RKS 记录结构，用于 RKS 计算与推分
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct RksRecord {
    /// 歌曲 ID（与定数表中的键一致）
    #[schema(example = "97f9466b2e77")]
    pub song_id: String,
    /// 难度枚举
    pub difficulty: Difficulty,
    /// 实际得分（如无可填 0）
    #[schema(example = 998765)]
    pub score: u32,
    /// ACC 百分比（例：98.50 表示 98.5%）
    #[schema(example = 98.73)]
    pub acc: f64,
    /// 该条成绩对应的 RKS 值
    #[schema(example = 13.21)]
    pub rks: f64,
    /// 该谱面的定数（constant）
    #[schema(example = 12.7)]
    pub chart_constant: f64,
}

/// 推分 ACC 计算结果（用于区分“无法推分”与“只能推到 100% 才能推分”）。
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
    pub fn target_acc(&self) -> Option<f64> {
        match self {
            Self::TargetAcc { acc } => Some(*acc),
            Self::PhiOnly | Self::Unreachable | Self::AlreadyPhi => None,
        }
    }

    /// 兼容旧逻辑：无法区分时以 100.0 表示“推到顶/无法推分”。
    pub fn as_legacy_acc(&self) -> f64 {
        match self {
            Self::TargetAcc { acc } => *acc,
            Self::PhiOnly | Self::Unreachable | Self::AlreadyPhi => 100.0,
        }
    }
}

fn target_rks_threshold_from_exact(current_exact_rks: f64) -> f64 {
    // 目标阈值（取决于第三位小数是否 >= 5）
    // 约定：目标是让「四舍五入到两位的显示 RKS」提升 0.01。
    let third_decimal_ge_5 = (current_exact_rks * 1000.0) % 10.0 >= 5.0;
    if third_decimal_ge_5 {
        (current_exact_rks * 100.0).floor() / 100.0 + 0.015
    } else {
        (current_exact_rks * 100.0).floor() / 100.0 + 0.005
    }
}

/// 推分 ACC 批量求解器：在同一份 records 上多次计算推分时复用预计算结果。
pub struct PushAccBatchSolver<'a> {
    records: &'a [RksRecord],
    target_rks_threshold: f64,

    // Best27/Best28 的和（records 已按 rks 降序）。
    total_rks_sum: f64,
    sum_first_27: f64,
    sum_first_28: f64,
    rks_27th: f64,
    rks_28th: f64,

    // AP 相关（按 rks 降序与 records 保持一致）。
    ap_rks: Box<[f64]>,
    ap_sum_3: f64,
    ap_sum_4: f64,
    ap_rank_by_index: Box<[Option<usize>]>,
}

impl<'a> PushAccBatchSolver<'a> {
    pub fn new(records: &'a [RksRecord]) -> Self {
        let (current_exact_rks, _rounded) = calculate_player_rks_details(records);
        let target_rks_threshold = target_rks_threshold_from_exact(current_exact_rks);

        let total_rks_sum: f64 = records.iter().map(|r| r.rks).sum();
        let sum_first_27: f64 = records.iter().take(27).map(|r| r.rks).sum();
        let sum_first_28: f64 = records.iter().take(28).map(|r| r.rks).sum();
        let rks_27th = records.get(26).map(|r| r.rks).unwrap_or(0.0);
        let rks_28th = records.get(27).map(|r| r.rks).unwrap_or(0.0);

        // AP 记录与 rank 映射
        let mut ap_rks_vec = Vec::<f64>::new();
        let mut ap_rank_by_index = vec![None; records.len()];
        for (idx, rec) in records.iter().enumerate() {
            if rec.acc >= 100.0 {
                ap_rank_by_index[idx] = Some(ap_rks_vec.len());
                ap_rks_vec.push(rec.rks);
            }
        }
        let ap_sum_3: f64 = ap_rks_vec.iter().take(3).sum();
        let ap_sum_4: f64 = ap_rks_vec.iter().take(4).sum();

        Self {
            records,
            target_rks_threshold,
            total_rks_sum,
            sum_first_27,
            sum_first_28,
            rks_27th,
            rks_28th,
            ap_rks: ap_rks_vec.into_boxed_slice(),
            ap_sum_3,
            ap_sum_4,
            ap_rank_by_index: ap_rank_by_index.into_boxed_slice(),
        }
    }

    /// 计算指定谱面（records 中的索引）所需达到的推分 ACC。
    ///
    /// - records 必须按 rks 降序；
    /// - 返回值区分三类：需要具体 ACC / 只能 100 / 无法推分。
    pub fn solve_for_index(
        &self,
        target_index: usize,
        target_chart_constant: f64,
    ) -> Option<PushAccHint> {
        let target = self.records.get(target_index)?;
        if self.records.is_empty() {
            return None;
        }
        // 定数异常或目标已满 ACC 时，推分提示没有意义（由上层决定是否展示）。
        if target_chart_constant <= 0.0 || target.acc >= 100.0 {
            return None;
        }

        let simulate = |test_acc: f64| -> f64 {
            let simulated_chart_rks = calculate_chart_rks(test_acc, target_chart_constant);

            // --- Best27 基底（排除目标谱面） ---
            let n = self.records.len();
            let target_rks = target.rks;
            let (b27_sum_excl, b27_count_excl, b27_min_excl) = if n <= 27 {
                // Best27 实际就是全量；排除后数量 < 27，插入时必然直接加入。
                (self.total_rks_sum - target_rks, n.saturating_sub(1), None)
            } else if target_index < 27 {
                // 目标在 Top27：使用前 28 条的和减去目标，27th 变为原 28th。
                (self.sum_first_28 - target_rks, 27, Some(self.rks_28th))
            } else {
                // 目标不在 Top27：Top27 不变。
                (self.sum_first_27, 27, Some(self.rks_27th))
            };

            let b27_sum_new = if b27_count_excl < 27 {
                b27_sum_excl + simulated_chart_rks
            } else if let Some(min_excl) = b27_min_excl
                && simulated_chart_rks > min_excl
            {
                b27_sum_excl - min_excl + simulated_chart_rks
            } else {
                b27_sum_excl
            };

            // --- AP Top3 基底（排除目标谱面） ---
            let ap_count = self.ap_rks.len();
            let target_is_ap = target.acc >= 100.0;

            let (ap_sum_excl, ap_count_excl, ap_min_excl) = if ap_count == 0 {
                (0.0, 0usize, None)
            } else if !target_is_ap {
                let cnt = ap_count.min(3);
                let min_excl = if cnt == 3 { Some(self.ap_rks[2]) } else { None };
                (self.ap_sum_3, cnt, min_excl)
            } else {
                // target 是 AP 成绩
                let Some(rank) = self.ap_rank_by_index.get(target_index).copied().flatten() else {
                    // 理论上不会发生：rank_by_index 与 ap_rks 来自同一份 records
                    return (b27_sum_new + self.ap_sum_3) / 30.0;
                };

                if ap_count <= 3 {
                    // AP 总数 <=3，排除后数量 <3，插入时直接加入。
                    (self.ap_sum_3 - target_rks, ap_count.saturating_sub(1), None)
                } else if rank < 3 {
                    // target 在 AP Top3：用 AP 前 4 条的和减去 target，AP3 的最小值变为原第 4 名。
                    (self.ap_sum_4 - target_rks, 3, Some(self.ap_rks[3]))
                } else {
                    // target 不在 AP Top3：AP3 不变。
                    (self.ap_sum_3, 3, Some(self.ap_rks[2]))
                }
            };

            let ap_sum_new = if test_acc >= 100.0 {
                if ap_count_excl < 3 {
                    ap_sum_excl + simulated_chart_rks
                } else if let Some(min_excl) = ap_min_excl
                    && simulated_chart_rks > min_excl
                {
                    ap_sum_excl - min_excl + simulated_chart_rks
                } else {
                    ap_sum_excl
                }
            } else {
                ap_sum_excl
            };

            (b27_sum_new + ap_sum_new) / 30.0
        };

        // 100% 时是否能达到目标（用于区分 Unreachable 与 PhiOnly）
        let rks_at_100 = simulate(100.0);
        if rks_at_100 < self.target_rks_threshold {
            return Some(PushAccHint::Unreachable);
        }

        // 结果最终只展示到 0.001 精度：用千分位整数二分可显著减少迭代次数。
        let mut low_i = (target.acc * 1000.0).ceil() as i64;
        if low_i < 0 {
            low_i = 0;
        }
        let high_i: i64 = 100_000;
        if low_i > high_i {
            low_i = high_i;
        }

        // 若低边界本身已可达，仍按“最小千分位”返回（避免浮点二分抖动）。
        let meets = |acc_thousand: i64| -> bool {
            let acc = (acc_thousand as f64) / 1000.0;
            simulate(acc) >= self.target_rks_threshold
        };

        let mut lo = low_i;
        let mut hi = high_i;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if meets(mid) {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        }

        if lo >= 100_000 {
            Some(PushAccHint::PhiOnly)
        } else {
            Some(PushAccHint::TargetAcc {
                acc: (lo as f64) / 1000.0,
            })
        }
    }
}

/// 为存档结构化成绩回填推分ACC（百分比）。
///
/// - 仅建议在需要返回/展示推分信息的场景调用（例如 /save?calculate_rks=true）。
/// - 结果使用 legacy 语义：无法推分/只能推到 100% 推分/已满 ACC 等场景统一回填 100.0。
pub fn fill_push_acc_for_game_record(game_record: &mut HashMap<String, Vec<DifficultyRecord>>) {
    // 1) 扁平化为引擎记录并按 rks 降序排序（PushAccBatchSolver 依赖排序）。
    let mut all: Vec<RksRecord> = Vec::new();
    for (song_id, diffs) in game_record.iter() {
        for rec in diffs.iter() {
            let Some(cc) = rec.chart_constant else {
                continue;
            };
            let mut acc_percent = rec.accuracy as f64;
            if acc_percent <= 1.5 {
                acc_percent *= 100.0;
            }
            let chart_constant = cc as f64;
            all.push(RksRecord {
                song_id: song_id.clone(),
                difficulty: rec.difficulty.clone(),
                score: rec.score,
                acc: acc_percent,
                rks: calculate_chart_rks(acc_percent, chart_constant),
                chart_constant,
            });
        }
    }
    all.sort_by(|a, b| {
        b.rks
            .partial_cmp(&a.rks)
            .unwrap_or(core::cmp::Ordering::Equal)
    });

    // 2) 预计算 solver，并对每个谱面求解推分提示（只对 acc<100 且定数有效者求解）。
    let solver = PushAccBatchSolver::new(&all);
    let mut hint_by_key: HashMap<String, PushAccHint> = HashMap::new();
    for (idx, rec) in all.iter().enumerate() {
        if rec.acc >= 100.0 || rec.chart_constant <= 0.0 {
            continue;
        }
        let key = format!("{}-{}", rec.song_id, rec.difficulty);
        if let Some(hint) = solver.solve_for_index(idx, rec.chart_constant) {
            hint_by_key.insert(key, hint);
        }
    }

    // 3) 回写到存档结构：为每条 DifficultyRecord 回填 push_acc + push_acc_hint（确保“每谱面都有值”）。
    for (song_id, diffs) in game_record.iter_mut() {
        for rec in diffs.iter_mut() {
            let key = format!("{}-{}", song_id, rec.difficulty);
            let mut acc_percent = rec.accuracy as f64;
            if acc_percent <= 1.5 {
                acc_percent *= 100.0;
            }
            if acc_percent >= 100.0 {
                rec.push_acc = Some(100.0);
                rec.push_acc_hint = Some(PushAccHint::AlreadyPhi);
                continue;
            }

            let Some(cc) = rec.chart_constant else {
                rec.push_acc = Some(100.0);
                rec.push_acc_hint = Some(PushAccHint::Unreachable);
                continue;
            };
            if (cc as f64) <= 0.0 {
                rec.push_acc = Some(100.0);
                rec.push_acc_hint = Some(PushAccHint::Unreachable);
                continue;
            }

            let hint = hint_by_key
                .get(&key)
                .copied()
                .unwrap_or(PushAccHint::Unreachable);
            rec.push_acc = Some(hint.as_legacy_acc());
            rec.push_acc_hint = Some(hint);
        }
    }
}

/// 计算玩家当前精确 RKS 与四舍五入后 RKS（要求 records 已按 rks 降序）
pub fn calculate_player_rks_details(records: &[RksRecord]) -> (f64, f64) {
    tracing::debug!("[B30 RKS] 开始计算玩家RKS详情，总成绩数: {}", records.len());

    if records.is_empty() {
        tracing::debug!("[B30 RKS] 无成绩记录，RKS = 0");
        return (0.0, 0.0);
    }

    let best_27_sum: f64 = records.iter().take(27).map(|r| r.rks).sum();
    let b27_count = records.len().min(27);
    tracing::debug!(
        "[B30 RKS] Best 27 计算: 使用 {} 个成绩，总和 = {:.4}",
        b27_count,
        best_27_sum
    );

    let ap_records = records.iter().filter(|r| r.acc >= 100.0);
    let ap_top_3_sum: f64 = ap_records.clone().take(3).map(|r| r.rks).sum();
    let ap3_count = records.iter().filter(|r| r.acc >= 100.0).count().min(3);
    tracing::debug!(
        "[B30 RKS] AP Top 3 计算: 使用 {} 个AP成绩，总和 = {:.4}",
        ap3_count,
        ap_top_3_sum
    );

    let final_exact_rks = (best_27_sum + ap_top_3_sum) / 30.0;
    let final_rounded_rks = (final_exact_rks * 100.0).round() / 100.0;

    tracing::debug!(
        "[B30 RKS] 最终 RKS 计算: exact {:.6} -> rounded {:.2}",
        final_exact_rks,
        final_rounded_rks
    );

    (final_exact_rks, final_rounded_rks)
}

/// 计算指定谱面的 RKS 值（acc 以百分比传入，如 98.5 表示 98.5%）
pub fn calculate_chart_rks(acc_percent: f64, constant: f64) -> f64 {
    if acc_percent < 70.0 {
        return 0.0;
    }
    let acc_factor = ((acc_percent - 55.0) / 45.0).powi(2);
    acc_factor * constant
}

fn split_chart_id_full(target_chart_id_full: &str) -> Option<(&str, &str)> {
    let (song_id, difficulty_str) = target_chart_id_full.rsplit_once('-')?;
    Some((song_id, difficulty_str))
}

fn difficulty_key_from_str(difficulty_str: &str) -> Option<u8> {
    if difficulty_str.eq_ignore_ascii_case("EZ") {
        return Some(0);
    }
    if difficulty_str.eq_ignore_ascii_case("HD") {
        return Some(1);
    }
    if difficulty_str.eq_ignore_ascii_case("IN") {
        return Some(2);
    }
    if difficulty_str.eq_ignore_ascii_case("AT") {
        return Some(3);
    }
    None
}

#[cfg(test)]
#[derive(Clone)]
struct TopKSum {
    k: usize,
    values: Vec<f64>,
    sum: f64,
}

#[cfg(test)]
impl TopKSum {
    fn new(k: usize) -> Self {
        Self {
            k,
            values: Vec::with_capacity(k),
            sum: 0.0,
        }
    }

    fn sum(&self) -> f64 {
        self.sum
    }

    fn push(&mut self, value: f64) {
        if self.k == 0 {
            return;
        }
        if self.values.len() < self.k {
            self.values.push(value);
            self.sum += value;
            return;
        }

        let min_index = self.min_index();
        let min_value = self.values[min_index];
        if Self::cmp_key(value).total_cmp(&Self::cmp_key(min_value)) == core::cmp::Ordering::Greater
        {
            self.values[min_index] = value;
            self.sum += value - min_value;
        }
    }

    fn min_index(&self) -> usize {
        debug_assert!(!self.values.is_empty());
        let mut min_index = 0;
        let mut min_key = Self::cmp_key(self.values[0]);
        for (idx, &v) in self.values.iter().enumerate().skip(1) {
            let key = Self::cmp_key(v);
            if key.total_cmp(&min_key) == core::cmp::Ordering::Less {
                min_index = idx;
                min_key = key;
            }
        }
        min_index
    }

    fn cmp_key(value: f64) -> f64 {
        if value.is_nan() {
            return f64::NEG_INFINITY;
        }
        value
    }
}

#[cfg(test)]
fn simulate_rks_increase_simplified_parsed(
    target_song_id: &str,
    target_difficulty_key: Option<u8>,
    target_chart_constant: f64,
    test_acc: f64,
    all_sorted_records: &[RksRecord],
) -> f64 {
    // 计算模拟后的该谱面 RKS
    let simulated_chart_rks = calculate_chart_rks(test_acc, target_chart_constant);

    // 只需要 Best27 与 AP Top3 的求和，无需构造完整 Vec 再全量排序
    let mut best27 = TopKSum::new(27);
    let mut ap3 = TopKSum::new(3);

    for rec in all_sorted_records {
        let is_target = rec.song_id == target_song_id
            && target_difficulty_key.is_some_and(|k| key_of_difficulty(&rec.difficulty) == k);
        if is_target {
            continue;
        }

        best27.push(rec.rks);
        if rec.acc >= 100.0 {
            ap3.push(rec.rks);
        }
    }

    // 插入新纪录
    best27.push(simulated_chart_rks);
    if test_acc >= 100.0 {
        ap3.push(simulated_chart_rks);
    }

    (best27.sum() + ap3.sum()) / 30.0
}

/// 计算指定谱面需要达到多少 ACC，才能让四舍五入后的玩家 RKS 提升 0.01
/// 返回需要达到的 ACC（百分比，最多 100.0）。
pub fn calculate_target_chart_push_acc(
    target_chart_id_full: &str,
    target_chart_constant: f64,
    all_sorted_records: &[RksRecord], // 需按 rks 降序
) -> Option<f64> {
    tracing::debug!("开始计算推分ACC: 目标谱面={}", target_chart_id_full);

    let Some((song_id, difficulty_str)) = split_chart_id_full(target_chart_id_full) else {
        tracing::debug!("推分ACC计算失败：目标谱面ID格式异常");
        return Some(100.0);
    };
    let difficulty_key = difficulty_key_from_str(difficulty_str);

    let Some(target_index) = all_sorted_records.iter().position(|r| {
        r.song_id == song_id
            && difficulty_key.is_some_and(|k| key_of_difficulty(&r.difficulty) == k)
    }) else {
        tracing::debug!("推分ACC计算失败：records 中未找到目标谱面");
        return Some(100.0);
    };

    let solver = PushAccBatchSolver::new(all_sorted_records);
    solver
        .solve_for_index(target_index, target_chart_constant)
        .map(|h| h.as_legacy_acc())
}

/// 批量计算给定（已按 rks 降序）的记录列表中每条非 100% 成绩的推分 ACC
/// 返回键为 `song_id-difficulty` 的映射（值为需要达到的 ACC 百分比）。
pub fn calculate_all_push_accuracies(sorted_records: &[RksRecord]) -> HashMap<String, f64> {
    let mut map = HashMap::new();
    let solver = PushAccBatchSolver::new(sorted_records);
    for (idx, rec) in sorted_records.iter().enumerate() {
        if rec.acc >= 100.0 {
            continue; // 已是满 ACC，无需推分
        }
        let chart_id = format!("{}-{}", rec.song_id, rec.difficulty);
        let hint = solver
            .solve_for_index(idx, rec.chart_constant)
            .unwrap_or(PushAccHint::PhiOnly);
        map.insert(chart_id, hint.as_legacy_acc());
    }
    map
}

/// 批量计算推分提示（区分 PhiOnly / Unreachable / 具体 ACC）。
pub fn calculate_all_push_hints(sorted_records: &[RksRecord]) -> HashMap<String, PushAccHint> {
    let mut map = HashMap::new();
    let solver = PushAccBatchSolver::new(sorted_records);
    for (idx, rec) in sorted_records.iter().enumerate() {
        if rec.acc >= 100.0 {
            continue;
        }
        let chart_id = format!("{}-{}", rec.song_id, rec.difficulty);
        if let Some(hint) = solver.solve_for_index(idx, rec.chart_constant) {
            map.insert(chart_id, hint);
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{Rng, SeedableRng};

    fn sort_records_desc(records: &mut [RksRecord]) {
        records.sort_by(|a, b| {
            b.rks
                .partial_cmp(&a.rks)
                .unwrap_or(core::cmp::Ordering::Equal)
        });
    }

    fn simulate_rks_increase_simplified_slow(
        target_chart_id_full: &str,
        target_chart_constant: f64,
        test_acc: f64,
        all_sorted_records: &[RksRecord],
    ) -> f64 {
        let parts: Vec<&str> = target_chart_id_full.rsplitn(2, '-').collect();
        if parts.len() != 2 {
            return 0.0; // 格式错误
        }
        let (song_id, difficulty_str) = (parts[1], parts[0]);

        let simulated_chart_rks = calculate_chart_rks(test_acc, target_chart_constant);

        let mut simulated_records: Vec<(f64, bool)> = all_sorted_records
            .iter()
            .filter(|r| !(r.song_id == song_id && r.difficulty.to_string() == difficulty_str))
            .map(|r| (r.rks, r.acc >= 100.0))
            .collect();

        simulated_records.push((simulated_chart_rks, test_acc >= 100.0));

        simulated_records
            .sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(core::cmp::Ordering::Equal));

        let b27_sum: f64 = simulated_records.iter().take(27).map(|(rks, _)| rks).sum();
        let ap3_sum: f64 = simulated_records
            .iter()
            .filter(|(_, is_ap)| *is_ap)
            .take(3)
            .map(|(rks, _)| rks)
            .sum();

        (b27_sum + ap3_sum) / 30.0
    }

    fn calculate_target_chart_push_acc_slow(
        target_chart_id_full: &str,
        target_chart_constant: f64,
        all_sorted_records: &[RksRecord],
    ) -> Option<f64> {
        let (current_exact_rks, _current_rounded_rks) =
            calculate_player_rks_details(all_sorted_records);

        let target_rks_threshold = target_rks_threshold_from_exact(current_exact_rks);

        let rks_at_100 = simulate_rks_increase_simplified_slow(
            target_chart_id_full,
            target_chart_constant,
            100.0,
            all_sorted_records,
        );

        if rks_at_100 < target_rks_threshold {
            return Some(100.0);
        }

        let parts: Vec<&str> = target_chart_id_full.rsplitn(2, '-').collect();
        if parts.len() != 2 {
            return None;
        }
        let (song_id, difficulty_str) = (parts[1], parts[0]);

        let current_acc = all_sorted_records
            .iter()
            .find(|r| r.song_id == song_id && r.difficulty.to_string() == difficulty_str)
            .map_or(70.0, |r| r.acc);

        let mut low = current_acc;
        let mut high = 100.0;

        const ACC_PRECISION: f64 = 1e-7;
        const MAX_ITERATIONS: usize = 50;

        let mut iteration = 0;
        while (high - low) > ACC_PRECISION && iteration < MAX_ITERATIONS {
            iteration += 1;
            let mid = low + (high - low) / 2.0;
            let simulated_rks = simulate_rks_increase_simplified_slow(
                target_chart_id_full,
                target_chart_constant,
                mid,
                all_sorted_records,
            );

            if simulated_rks >= target_rks_threshold {
                high = mid;
            } else {
                low = mid;
            }
        }

        let result_acc = high.max(current_acc);
        let final_acc = if result_acc <= current_acc {
            100.0
        } else {
            (result_acc * 1000.0).ceil() / 1000.0
        };

        Some(final_acc.min(100.0))
    }

    fn calculate_target_chart_push_hint_slow(
        target_chart_id_full: &str,
        target_chart_constant: f64,
        all_sorted_records: &[RksRecord],
    ) -> Option<PushAccHint> {
        let (current_exact_rks, _current_rounded_rks) =
            calculate_player_rks_details(all_sorted_records);
        let target_rks_threshold = target_rks_threshold_from_exact(current_exact_rks);

        // 100% 时是否能达到目标（用于区分 Unreachable 与 PhiOnly）
        let rks_at_100 = simulate_rks_increase_simplified_slow(
            target_chart_id_full,
            target_chart_constant,
            100.0,
            all_sorted_records,
        );
        if rks_at_100 < target_rks_threshold {
            return Some(PushAccHint::Unreachable);
        }

        let parts: Vec<&str> = target_chart_id_full.rsplitn(2, '-').collect();
        if parts.len() != 2 {
            return None;
        }
        let (song_id, difficulty_str) = (parts[1], parts[0]);
        let current_acc = all_sorted_records
            .iter()
            .find(|r| r.song_id == song_id && r.difficulty.to_string() == difficulty_str)
            .map_or(70.0, |r| r.acc);

        let mut low_i = (current_acc * 1000.0).ceil() as i64;
        if low_i < 0 {
            low_i = 0;
        }
        let high_i: i64 = 100_000;
        if low_i > high_i {
            low_i = high_i;
        }

        let meets = |acc_thousand: i64| -> bool {
            let acc = (acc_thousand as f64) / 1000.0;
            simulate_rks_increase_simplified_slow(
                target_chart_id_full,
                target_chart_constant,
                acc,
                all_sorted_records,
            ) >= target_rks_threshold
        };

        let mut lo = low_i;
        let mut hi = high_i;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if meets(mid) {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        }

        if lo >= 100_000 {
            Some(PushAccHint::PhiOnly)
        } else {
            Some(PushAccHint::TargetAcc {
                acc: (lo as f64) / 1000.0,
            })
        }
    }

    #[test]
    fn simulate_rks_increase_simplified_matches_reference() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(20251215);
        let diffs = [
            Difficulty::EZ,
            Difficulty::HD,
            Difficulty::IN,
            Difficulty::AT,
        ];

        let mut records = Vec::new();
        for i in 0..60 {
            let song_id = format!("song{i:03}");
            for diff in diffs.iter().cloned() {
                let acc = rng.gen_range(70.0..=100.0);
                let constant = rng.gen_range(1.0..=16.0);
                let rks = calculate_chart_rks(acc, constant);
                records.push(RksRecord {
                    song_id: song_id.clone(),
                    difficulty: diff,
                    score: rng.gen_range(0..=1_000_000),
                    acc,
                    rks,
                    chart_constant: constant,
                });
            }
        }
        sort_records_desc(&mut records);

        // 挑一个非 100% 的谱面做目标
        let target = records
            .iter()
            .find(|r| r.acc < 100.0)
            .expect("需要至少一个非满ACC记录");
        let target_chart_id = format!("{}-{}", target.song_id, target.difficulty);
        let target_constant = target.chart_constant;
        let (target_song_id, target_difficulty_str) =
            split_chart_id_full(&target_chart_id).expect("目标谱面ID应当符合 song_id-difficulty");
        let target_difficulty_key = difficulty_key_from_str(target_difficulty_str);

        for test_acc in [70.0, 73.3, 80.0, 90.5, 99.999, 100.0] {
            let slow = simulate_rks_increase_simplified_slow(
                &target_chart_id,
                target_constant,
                test_acc,
                &records,
            );
            let fast = simulate_rks_increase_simplified_parsed(
                target_song_id,
                target_difficulty_key,
                target_constant,
                test_acc,
                &records,
            );
            assert!(
                (slow - fast).abs() <= 1e-12,
                "slow={slow:.12}, fast={fast:.12}, acc={test_acc}"
            );
        }
    }

    #[test]
    fn calculate_target_chart_push_acc_matches_reference() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(2025121501);
        let diffs = [
            Difficulty::EZ,
            Difficulty::HD,
            Difficulty::IN,
            Difficulty::AT,
        ];

        let mut records = Vec::new();
        for i in 0..80 {
            let song_id = format!("song{i:03}");
            for diff in diffs.iter().cloned() {
                let acc = rng.gen_range(70.0..=100.0);
                let constant = rng.gen_range(1.0..=16.0);
                let rks = calculate_chart_rks(acc, constant);
                records.push(RksRecord {
                    song_id: song_id.clone(),
                    difficulty: diff,
                    score: rng.gen_range(0..=1_000_000),
                    acc,
                    rks,
                    chart_constant: constant,
                });
            }
        }
        sort_records_desc(&mut records);

        // 抽样多个目标谱面做对拍
        let targets: Vec<_> = records.iter().filter(|r| r.acc < 100.0).take(12).collect();
        assert!(!targets.is_empty(), "需要至少一个非满ACC记录");

        for t in targets {
            let target_chart_id = format!("{}-{}", t.song_id, t.difficulty);
            let slow =
                calculate_target_chart_push_acc_slow(&target_chart_id, t.chart_constant, &records);
            let fast =
                calculate_target_chart_push_acc(&target_chart_id, t.chart_constant, &records);
            assert_eq!(slow, fast, "target={target_chart_id}");
        }
    }

    #[test]
    fn calculate_target_chart_push_hint_matches_reference() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(2026010601);
        let diffs = [
            Difficulty::EZ,
            Difficulty::HD,
            Difficulty::IN,
            Difficulty::AT,
        ];

        let mut records = Vec::new();
        for i in 0..90 {
            let song_id = format!("song{i:03}");
            for diff in diffs.iter().cloned() {
                let acc = rng.gen_range(70.0..=100.0);
                let constant = rng.gen_range(1.0..=16.0);
                let rks = calculate_chart_rks(acc, constant);
                records.push(RksRecord {
                    song_id: song_id.clone(),
                    difficulty: diff,
                    score: rng.gen_range(0..=1_000_000),
                    acc,
                    rks,
                    chart_constant: constant,
                });
            }
        }
        sort_records_desc(&mut records);

        let solver = PushAccBatchSolver::new(&records);
        let targets: Vec<_> = records
            .iter()
            .enumerate()
            .filter(|(_, r)| r.acc < 100.0)
            .take(16)
            .collect();
        assert!(!targets.is_empty(), "需要至少一个非满ACC记录");

        for (idx, t) in targets {
            let target_chart_id = format!("{}-{}", t.song_id, t.difficulty);
            let slow =
                calculate_target_chart_push_hint_slow(&target_chart_id, t.chart_constant, &records);
            let fast = solver.solve_for_index(idx, t.chart_constant);
            assert_eq!(slow, fast, "target={target_chart_id}");
        }
    }

    #[test]
    fn push_acc_hint_covers_all_kinds() {
        // 目标：用可控数据确保三类结果都能出现：
        // - TargetAcc：提升到 <100 的具体 ACC 即可推分
        // - PhiOnly：只有到 100.0% 才能推分
        // - Unreachable：即使 100.0% 也无法推分

        // --- 1) TargetAcc ---
        // 27 条记录，无 AP；其中一条 ACC=70，可通过小幅提升 ACC 推分。
        let mut records_target_acc = Vec::<RksRecord>::new();
        for i in 0..26 {
            let acc = 99.0;
            let constant = 12.0;
            records_target_acc.push(RksRecord {
                song_id: format!("hi{i:02}"),
                difficulty: Difficulty::IN,
                score: 900_000,
                acc,
                rks: calculate_chart_rks(acc, constant),
                chart_constant: constant,
            });
        }
        {
            let acc = 70.0;
            let constant = 12.0;
            records_target_acc.push(RksRecord {
                song_id: "target-acc".into(),
                difficulty: Difficulty::IN,
                score: 800_000,
                acc,
                rks: calculate_chart_rks(acc, constant),
                chart_constant: constant,
            });
        }
        sort_records_desc(&mut records_target_acc);
        let solver = PushAccBatchSolver::new(&records_target_acc);
        let idx = records_target_acc
            .iter()
            .position(|r| r.song_id == "target-acc")
            .expect("目标谱面应存在");
        let hint = solver
            .solve_for_index(idx, records_target_acc[idx].chart_constant)
            .expect("应可计算推分提示");
        assert!(
            matches!(hint, PushAccHint::TargetAcc { acc } if acc < 100.0),
            "期望 TargetAcc(<100)，实际={hint:?}"
        );

        // --- 2) PhiOnly ---
        // 无 AP 且 target 不在 Best27：100% 时因进入 AP Top3 才能推分。
        let mut records_phi_only = Vec::<RksRecord>::new();
        for i in 0..29 {
            let acc = 99.0;
            let constant = 12.0;
            records_phi_only.push(RksRecord {
                song_id: format!("hi{i:02}"),
                difficulty: Difficulty::IN,
                score: 900_000,
                acc,
                rks: calculate_chart_rks(acc, constant),
                chart_constant: constant,
            });
        }
        {
            let acc = 99.0;
            let constant = 1.0;
            records_phi_only.push(RksRecord {
                song_id: "phi-only".into(),
                difficulty: Difficulty::IN,
                score: 700_000,
                acc,
                rks: calculate_chart_rks(acc, constant),
                chart_constant: constant,
            });
        }
        sort_records_desc(&mut records_phi_only);
        let solver = PushAccBatchSolver::new(&records_phi_only);
        let idx = records_phi_only
            .iter()
            .position(|r| r.song_id == "phi-only")
            .expect("目标谱面应存在");
        let hint = solver
            .solve_for_index(idx, records_phi_only[idx].chart_constant)
            .expect("应可计算推分提示");
        assert_eq!(hint, PushAccHint::PhiOnly, "期望 PhiOnly，实际={hint:?}");

        // --- 3) Unreachable ---
        // AP Top3 已被高 rks 填满，且 target 无法进入 Best27/AP3：即使 100 也无法推分。
        let mut records_unreachable = Vec::<RksRecord>::new();
        for i in 0..27 {
            let acc = 99.0;
            let constant = 12.0;
            records_unreachable.push(RksRecord {
                song_id: format!("hi{i:02}"),
                difficulty: Difficulty::IN,
                score: 900_000,
                acc,
                rks: calculate_chart_rks(acc, constant),
                chart_constant: constant,
            });
        }
        for i in 0..3 {
            let acc = 100.0;
            let constant = 16.0;
            records_unreachable.push(RksRecord {
                song_id: format!("ap{i:02}"),
                difficulty: Difficulty::IN,
                score: 1_000_000,
                acc,
                rks: calculate_chart_rks(acc, constant),
                chart_constant: constant,
            });
        }
        {
            let acc = 99.0;
            let constant = 1.0;
            records_unreachable.push(RksRecord {
                song_id: "unreachable".into(),
                difficulty: Difficulty::IN,
                score: 700_000,
                acc,
                rks: calculate_chart_rks(acc, constant),
                chart_constant: constant,
            });
        }
        sort_records_desc(&mut records_unreachable);
        let solver = PushAccBatchSolver::new(&records_unreachable);
        let idx = records_unreachable
            .iter()
            .position(|r| r.song_id == "unreachable")
            .expect("目标谱面应存在");
        let hint = solver
            .solve_for_index(idx, records_unreachable[idx].chart_constant)
            .expect("应可计算推分提示");
        assert_eq!(
            hint,
            PushAccHint::Unreachable,
            "期望 Unreachable，实际={hint:?}"
        );
    }

    #[test]
    fn fill_push_acc_for_game_record_fills_values() {
        let mut game_record: HashMap<String, Vec<DifficultyRecord>> = HashMap::new();
        game_record.insert(
            "song_a".to_string(),
            vec![
                DifficultyRecord {
                    difficulty: Difficulty::IN,
                    score: 900_000,
                    accuracy: 95.0,
                    is_full_combo: false,
                    chart_constant: Some(12.0),
                    push_acc: None,
                    push_acc_hint: None,
                },
                // 小数语义：0.985 => 98.5%
                DifficultyRecord {
                    difficulty: Difficulty::EZ,
                    score: 800_000,
                    accuracy: 0.985,
                    is_full_combo: false,
                    chart_constant: Some(3.0),
                    push_acc: None,
                    push_acc_hint: None,
                },
            ],
        );
        game_record.insert(
            "song_b".to_string(),
            vec![
                // 已满ACC：按 legacy 语义回填 100.0
                DifficultyRecord {
                    difficulty: Difficulty::AT,
                    score: 1_000_000,
                    accuracy: 100.0,
                    is_full_combo: true,
                    chart_constant: Some(15.0),
                    push_acc: None,
                    push_acc_hint: None,
                },
                // 缺定数：按 legacy 语义回填 100.0
                DifficultyRecord {
                    difficulty: Difficulty::HD,
                    score: 700_000,
                    accuracy: 90.0,
                    is_full_combo: false,
                    chart_constant: None,
                    push_acc: None,
                    push_acc_hint: None,
                },
            ],
        );

        fill_push_acc_for_game_record(&mut game_record);

        for (song_id, diffs) in game_record.iter() {
            for rec in diffs {
                let Some(push) = rec.push_acc else {
                    panic!(
                        "push_acc 未回填: song_id={song_id}, diff={}",
                        rec.difficulty
                    );
                };
                let Some(hint) = rec.push_acc_hint else {
                    panic!(
                        "push_acc_hint 未回填: song_id={song_id}, diff={}",
                        rec.difficulty
                    );
                };
                assert!(push <= 100.0, "push_acc 应 <=100: {push}");
                let mut current = rec.accuracy as f64;
                if current <= 1.5 {
                    current *= 100.0;
                }
                assert!(
                    push >= current,
                    "push_acc 应 >= 当前ACC: push={push}, current={current}, song_id={song_id}, diff={}",
                    rec.difficulty
                );

                match hint {
                    PushAccHint::TargetAcc { acc } => {
                        assert_eq!(push, acc, "TargetAcc 时 push_acc 应等于目标ACC");
                    }
                    PushAccHint::PhiOnly | PushAccHint::Unreachable | PushAccHint::AlreadyPhi => {
                        assert_eq!(push, 100.0, "非 TargetAcc 时 push_acc 应为 100.0");
                    }
                }
            }
        }

        let song_b = game_record.get("song_b").expect("song_b 应存在");
        let at = song_b
            .iter()
            .find(|r| r.difficulty == Difficulty::AT)
            .expect("song_b AT 应存在");
        assert_eq!(at.push_acc, Some(100.0));
        assert_eq!(at.push_acc_hint, Some(PushAccHint::AlreadyPhi));

        let hd = song_b
            .iter()
            .find(|r| r.difficulty == Difficulty::HD)
            .expect("song_b HD 应存在");
        assert_eq!(hd.push_acc, Some(100.0));
        assert_eq!(hd.push_acc_hint, Some(PushAccHint::Unreachable));
    }

    #[test]
    fn push_acc_hint_serializes_tagged_enum() {
        use serde_json::json;

        assert_eq!(
            serde_json::to_value(PushAccHint::Unreachable).expect("serialize"),
            json!({"type": "unreachable"})
        );
        assert_eq!(
            serde_json::to_value(PushAccHint::PhiOnly).expect("serialize"),
            json!({"type": "phi_only"})
        );
        assert_eq!(
            serde_json::to_value(PushAccHint::AlreadyPhi).expect("serialize"),
            json!({"type": "already_phi"})
        );
        assert_eq!(
            serde_json::to_value(PushAccHint::TargetAcc { acc: 98.123 }).expect("serialize"),
            json!({"type": "target_acc", "acc": 98.123})
        );
    }

    #[test]
    fn difficulty_record_serializes_push_acc_hint_field() {
        use serde_json::json;

        let rec = DifficultyRecord {
            difficulty: Difficulty::IN,
            score: 900_000,
            accuracy: 95.0,
            is_full_combo: false,
            chart_constant: Some(12.0),
            push_acc: Some(100.0),
            push_acc_hint: Some(PushAccHint::PhiOnly),
        };

        let v = serde_json::to_value(&rec).expect("serialize");
        assert_eq!(v.get("push_acc_hint"), Some(&json!({"type": "phi_only"})));
    }
}
