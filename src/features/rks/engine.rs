use std::collections::HashMap;

use crate::features::save::models::{Difficulty, DifficultyRecord};
use crate::startup::chart_loader::{ChartConstants, ChartConstantsMap};
use serde::Serialize;

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

#[derive(Clone)]
struct TopKSum {
    k: usize,
    values: Vec<f64>,
    sum: f64,
}

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

    // 当前精确 RKS
    let (current_exact_rks, _current_rounded_rks) =
        calculate_player_rks_details(all_sorted_records);

    // 目标阈值（取决于第三位小数是否 >= 5）
    let target_rks_threshold = {
        let third_decimal_ge_5 = (current_exact_rks * 1000.0) % 10.0 >= 5.0;
        if third_decimal_ge_5 {
            (current_exact_rks * 100.0).floor() / 100.0 + 0.015
        } else {
            (current_exact_rks * 100.0).floor() / 100.0 + 0.005
        }
    };

    if current_exact_rks >= target_rks_threshold {
        tracing::debug!("无需推分，当前 RKS 已达标");
        return Some(100.0);
    }

    let Some((song_id, difficulty_str)) = split_chart_id_full(target_chart_id_full) else {
        // 保持与旧实现一致：格式异常时模拟函数返回 0.0，必然无法达到阈值
        tracing::debug!("无法推分，ACC 100% 仍无法达到目标");
        return Some(100.0);
    };
    let difficulty_key = difficulty_key_from_str(difficulty_str);

    // 性能优化：二分迭代会重复进行“模拟后 RKS”计算。
    // 原实现每次模拟都会遍历整份 records 计算 Best27/AP3（O(N)），在最多 50 次迭代下会放大为 O(50N)。
    // 这里先计算“排除目标谱面后的 Best27/AP3 基底”，迭代时只需 clone 小型 TopKSum 并插入模拟值（O(K)）。
    // 该优化不改变任何计算逻辑与结果（等价变换）。
    let mut best27_base = TopKSum::new(27);
    let mut ap3_base = TopKSum::new(3);
    for rec in all_sorted_records {
        let is_target = rec.song_id == song_id
            && difficulty_key.is_some_and(|k| key_of_difficulty(&rec.difficulty) == k);
        if is_target {
            continue;
        }
        best27_base.push(rec.rks);
        if rec.acc >= 100.0 {
            ap3_base.push(rec.rks);
        }
    }

    let simulate_from_base = |test_acc: f64| -> f64 {
        let simulated_chart_rks = calculate_chart_rks(test_acc, target_chart_constant);
        let mut best27 = best27_base.clone();
        best27.push(simulated_chart_rks);
        let mut ap3 = ap3_base.clone();
        if test_acc >= 100.0 {
            ap3.push(simulated_chart_rks);
        }
        (best27.sum() + ap3.sum()) / 30.0
    };

    // 边界检查：100% 时是否能达到目标
    let rks_at_100 = simulate_from_base(100.0);

    if rks_at_100 < target_rks_threshold {
        tracing::debug!("无法推分，ACC 100% 仍无法达到目标");
        return Some(100.0);
    }

    let current_acc = all_sorted_records
        .iter()
        .find(|r| {
            r.song_id == song_id
                && difficulty_key.is_some_and(|k| key_of_difficulty(&r.difficulty) == k)
        })
        .map_or(70.0, |r| r.acc);

    // 二分查找最小满足阈值的 ACC
    let mut low = current_acc;
    let mut high = 100.0;
    tracing::debug!("开始二分查找推分ACC, 区间: [{:.4}, {:.4}]", low, high);

    const ACC_PRECISION: f64 = 1e-7; // 精度 ~0.00001%
    const MAX_ITERATIONS: usize = 50;

    let mut iteration = 0;
    while (high - low) > ACC_PRECISION && iteration < MAX_ITERATIONS {
        iteration += 1;
        let mid = low + (high - low) / 2.0;
        let simulated_rks = simulate_from_base(mid);

        if simulated_rks >= target_rks_threshold {
            high = mid; // mid 满足条件，尝试更低的 acc
        } else {
            low = mid; // 需要更高的 acc
        }

        tracing::debug!(
            "迭代 {}: 区间 [{:.8}, {:.8}], 区间长度: {:.8}",
            iteration,
            low,
            high,
            high - low
        );
    }

    tracing::debug!(
        "二分查找结束, 迭代次数: {}, 区间长度: {:.8}, 结果 high = {:.8}",
        iteration,
        high - low,
        high
    );

    // 确保结果不小于当前 ACC，并保留到小数点后三位（向上取）
    let result_acc = high.max(current_acc);
    let final_acc = if result_acc <= current_acc {
        tracing::debug!(
            "推分ACC计算结果({:.6})不大于当前ACC({:.6})，返回 100.0",
            result_acc,
            current_acc
        );
        100.0
    } else {
        (result_acc * 1000.0).ceil() / 1000.0
    };

    Some(final_acc.min(100.0))
}

/// 批量计算给定（已按 rks 降序）的记录列表中每条非 100% 成绩的推分 ACC
/// 返回键为 `song_id-difficulty` 的映射（值为需要达到的 ACC 百分比）。
pub fn calculate_all_push_accuracies(sorted_records: &[RksRecord]) -> HashMap<String, f64> {
    let mut map = HashMap::new();
    for rec in sorted_records {
        if rec.acc >= 100.0 {
            continue; // 已是满 ACC，无需推分
        }
        let chart_id = format!("{}-{}", rec.song_id, rec.difficulty);
        if let Some(target_acc) =
            calculate_target_chart_push_acc(&chart_id, rec.chart_constant, sorted_records)
        {
            map.insert(chart_id, target_acc);
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

        let target_rks_threshold = {
            let third_decimal_ge_5 = (current_exact_rks * 1000.0) % 10.0 >= 5.0;
            if third_decimal_ge_5 {
                (current_exact_rks * 100.0).floor() / 100.0 + 0.015
            } else {
                (current_exact_rks * 100.0).floor() / 100.0 + 0.005
            }
        };

        if current_exact_rks >= target_rks_threshold {
            return Some(100.0);
        }

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
}
