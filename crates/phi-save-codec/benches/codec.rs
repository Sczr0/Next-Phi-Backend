//! 存档编解码基准测试（`divan` + `CodSpeed`）。
//!
//! 覆盖存档解析链路上的纯计算热点：gameRecord 二进制解析、gameRecord JSON
//! 解析与 summary（base64）解析。输入数据按真实二进制格式合成，规模固定可复现。
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]

use std::collections::HashMap;

use base64::Engine as _;
use divan::{Bencher, black_box};
use phi_save_codec::parse_summary_base64;
use phi_save_codec::{Difficulty, parse_game_record_bytes, parse_game_record_json};
use serde_json::{Map, Value};

fn main() {
    divan::main();
}

/// 存档规模（歌曲数）：小号 / 典型玩家 / 全曲库玩家。
const SONG_COUNTS: &[usize] = &[50, 300, 600];

/// 单首歌的合成成绩：(score, acc) 按 EZ/HD/IN/AT 顺序，`None` 表示无该难度。
type SongScores = [Option<(u32, f32)>; 4];

fn song_id(i: usize) -> String {
    format!("song{i:04}.Composer{}", i % 37)
}

fn song_scores(i: usize) -> SongScores {
    let acc = |k: usize| 80.0 + ((i * 7 + k * 13) % 2000) as f32 / 100.0;
    let entry = |k: usize| {
        let a = acc(k);
        Some(((a / 100.0 * 1_000_000.0) as u32, a))
    };
    [
        entry(0),
        entry(1),
        entry(2),
        if i % 5 < 2 { entry(3) } else { None },
    ]
}

fn push_varshort(buf: &mut Vec<u8>, v: usize) {
    if v < 0x80 {
        buf.push(v as u8);
    } else {
        buf.push(((v & 0x7F) | 0x80) as u8);
        buf.push(((v >> 7) & 0xFF) as u8);
    }
}

/// 按存档二进制格式构造完整 gameRecord entry（含 1 字节前缀）。
fn build_game_record_entry(song_count: usize) -> Vec<u8> {
    let mut entry = vec![0u8];
    push_varshort(&mut entry, song_count);
    for i in 0..song_count {
        // song_id 末尾带 2 字节后缀（解析时裁掉）
        let key = format!("{}.0", song_id(i));
        push_varshort(&mut entry, key.len());
        entry.extend_from_slice(key.as_bytes());

        let scores = song_scores(i);
        let mut mask = 0u8;
        let mut fc_mask = 0u8;
        let mut payload = Vec::with_capacity(34);
        for (idx, s) in scores.iter().enumerate() {
            if let Some((score, acc)) = s {
                mask |= 1 << idx;
                if (i + idx) % 3 == 0 {
                    fc_mask |= 1 << idx;
                }
                payload.extend_from_slice(&(*score as i32).to_le_bytes());
                payload.extend_from_slice(&acc.to_le_bytes());
            }
        }
        entry.push((payload.len() + 2) as u8);
        entry.push(mask);
        entry.push(fc_mask);
        entry.extend_from_slice(&payload);
    }
    entry
}

/// 构造与 `LeanCloud` 返回格式一致的 `gameRecord` JSON（每难度 [score, acc, fc] 三元组）。
fn build_game_record_json(song_count: usize) -> Value {
    let mut obj = Map::new();
    for i in 0..song_count {
        let mut arr = Vec::with_capacity(12);
        for (idx, s) in song_scores(i).iter().enumerate() {
            let (score, acc) = s.unwrap_or((0, 0.0));
            arr.push(Value::from(i64::from(score)));
            arr.push(Value::from(f64::from(acc)));
            arr.push(Value::from(i64::from((i + idx) % 3 == 0)));
        }
        obj.insert(song_id(i), Value::Array(arr));
    }
    Value::Object(obj)
}

fn build_chart_constants(song_count: usize) -> HashMap<String, [Option<f32>; 4]> {
    (0..song_count)
        .map(|i| {
            let base = 1.0 + (i % 40) as f32 / 10.0;
            (
                song_id(i),
                [
                    Some(base),
                    Some(base + 4.0),
                    Some(base + 9.0),
                    (i % 5 < 2).then_some(base + 12.0),
                ],
            )
        })
        .collect()
}

const fn difficulty_index(d: Difficulty) -> usize {
    match d {
        Difficulty::EZ => 0,
        Difficulty::HD => 1,
        Difficulty::IN => 2,
        Difficulty::AT => 3,
    }
}

#[divan::bench(args = SONG_COUNTS)]
fn game_record_bytes(bencher: Bencher, song_count: usize) {
    let entry = build_game_record_entry(song_count);
    let constants = build_chart_constants(song_count);
    bencher.bench_local(|| {
        parse_game_record_bytes(black_box(&entry), |id, d| {
            constants.get(id).and_then(|c| c[difficulty_index(d)])
        })
    });
}

#[divan::bench(args = SONG_COUNTS)]
fn game_record_json(bencher: Bencher, song_count: usize) {
    let value = build_game_record_json(song_count);
    let constants = build_chart_constants(song_count);
    bencher.bench_local(|| {
        parse_game_record_json(black_box(&value), |id, d| {
            constants.get(id).and_then(|c| c[difficulty_index(d)])
        })
    });
}

#[divan::bench]
fn summary_base64(bencher: Bencher) {
    let mut raw = Vec::new();
    raw.push(6u8); // save_version
    raw.extend_from_slice(&345u16.to_le_bytes()); // challenge_mode_rank
    raw.extend_from_slice(&15.67f32.to_le_bytes()); // ranking_score
    raw.push(90u8); // game_version
    let avatar = "Introduction";
    push_varshort(&mut raw, avatar.len());
    raw.extend_from_slice(avatar.as_bytes());
    for p in 0..12u16 {
        raw.extend_from_slice(&(p * 17).to_le_bytes());
    }
    let encoded = base64::engine::general_purpose::STANDARD.encode(&raw);
    bencher.bench_local(|| parse_summary_base64(black_box(&encoded)));
}
