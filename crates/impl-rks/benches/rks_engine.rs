//! RKS 引擎基准测试（`divan` + `CodSpeed`）。
//!
//! 覆盖存档查询链路上的纯计算热点：B30/RKS 计算、推分 ACC 批量求解、
//! 存档回填推分提示。输入数据由固定种子的伪随机生成器构造，保证结果可复现。
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use std::collections::HashMap;

use divan::{Bencher, black_box};
use impl_rks::engine::{
    PushAccBatchSolver, RksRecord, calculate_all_push_accuracies, calculate_all_push_hints,
    calculate_chart_rks, calculate_player_rks, calculate_player_rks_details,
    fill_push_acc_for_game_record,
};
use impl_rks::save_contract::{Difficulty, DifficultyRecord};
use impl_rks::startup::chart_loader::{ChartConstants, ChartConstantsMap};

fn main() {
    divan::main();
}

/// 存档规模（歌曲数）：小号 / 典型玩家 / 全曲库玩家。
const SONG_COUNTS: &[usize] = &[50, 300, 600];

const DIFFICULTIES: [Difficulty; 4] = [
    Difficulty::EZ,
    Difficulty::HD,
    Difficulty::IN,
    Difficulty::AT,
];

/// 简单的 xorshift 伪随机数生成器（固定种子，避免基准输入抖动）。
struct Rng(u64);

impl Rng {
    const fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// [0, 1) 区间的浮点数。
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

struct Fixture {
    game_record: HashMap<String, Vec<DifficultyRecord>>,
    chart_constants: ChartConstantsMap,
}

fn build_fixture(song_count: usize) -> Fixture {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ song_count as u64);
    let mut game_record = HashMap::with_capacity(song_count);
    let mut chart_constants = ChartConstantsMap::with_capacity(song_count);

    for i in 0..song_count {
        let song_id = format!("song{i:04}.Composer{}", i % 37);
        let base = 1.0 + rng.next_f64() * 4.0;
        let has_at = rng.next_f64() < 0.4;
        let levels: [Option<f32>; 4] = [
            Some(base as f32),
            Some((base + 3.0 + rng.next_f64() * 2.0) as f32),
            Some((base + 7.0 + rng.next_f64() * 4.0) as f32),
            has_at.then(|| (base + 11.0 + rng.next_f64() * 2.5) as f32),
        ];
        chart_constants.insert(
            song_id.clone(),
            ChartConstants {
                ez: levels[0],
                hd: levels[1],
                in_level: levels[2],
                at: levels[3],
            },
        );

        let mut diffs = Vec::with_capacity(4);
        for (idx, difficulty) in DIFFICULTIES.iter().enumerate() {
            let Some(level) = levels[idx] else {
                continue;
            };
            // 约 15% 的谱面为 AP，其余 ACC 分布在 [80, 100)。
            let accuracy = if rng.next_f64() < 0.15 {
                100.0
            } else {
                80.0 + rng.next_f64() * 19.99
            };
            let score = (accuracy / 100.0 * 1_000_000.0) as u32;
            diffs.push(DifficultyRecord {
                difficulty: *difficulty,
                score,
                accuracy: accuracy as f32,
                is_full_combo: accuracy >= 100.0 || rng.next_f64() < 0.3,
                chart_constant: Some(level),
                push_acc: None,
                push_acc_hint: None,
            });
        }
        game_record.insert(song_id, diffs);
    }

    Fixture {
        game_record,
        chart_constants,
    }
}

/// 扁平化并按 rks 降序排序（推分求解器的前置条件）。
fn build_sorted_records(song_count: usize) -> Vec<RksRecord> {
    let fixture = build_fixture(song_count);
    let mut records: Vec<RksRecord> = fixture
        .game_record
        .iter()
        .flat_map(|(song_id, diffs)| {
            diffs.iter().filter_map(move |rec| {
                let chart_constant = f64::from(rec.chart_constant?);
                let acc = f64::from(rec.accuracy);
                Some(RksRecord {
                    song_id: song_id.clone(),
                    difficulty: rec.difficulty,
                    score: rec.score,
                    acc,
                    rks: calculate_chart_rks(acc, chart_constant),
                    chart_constant,
                })
            })
        })
        .collect();
    records.sort_by(|a, b| b.rks.total_cmp(&a.rks));
    records
}

#[divan::bench(args = SONG_COUNTS)]
fn player_rks(bencher: Bencher, song_count: usize) {
    let fixture = build_fixture(song_count);
    bencher.bench_local(|| {
        calculate_player_rks(
            black_box(&fixture.game_record),
            black_box(&fixture.chart_constants),
        )
    });
}

#[divan::bench(args = SONG_COUNTS)]
fn player_rks_details(bencher: Bencher, song_count: usize) {
    let records = build_sorted_records(song_count);
    bencher.bench_local(|| calculate_player_rks_details(black_box(&records)));
}

#[divan::bench(args = SONG_COUNTS)]
fn push_acc_solver_new(bencher: Bencher, song_count: usize) {
    let records = build_sorted_records(song_count);
    bencher.bench_local(|| PushAccBatchSolver::new(black_box(&records)));
}

#[divan::bench(args = SONG_COUNTS)]
fn all_push_hints(bencher: Bencher, song_count: usize) {
    let records = build_sorted_records(song_count);
    bencher.bench_local(|| calculate_all_push_hints(black_box(&records)));
}

#[divan::bench(args = SONG_COUNTS)]
fn all_push_accuracies(bencher: Bencher, song_count: usize) {
    let records = build_sorted_records(song_count);
    bencher.bench_local(|| calculate_all_push_accuracies(black_box(&records)));
}

#[divan::bench(args = SONG_COUNTS)]
fn fill_push_acc(bencher: Bencher, song_count: usize) {
    let fixture = build_fixture(song_count);
    bencher
        .with_inputs(|| fixture.game_record.clone())
        .bench_local_refs(fill_push_acc_for_game_record);
}
