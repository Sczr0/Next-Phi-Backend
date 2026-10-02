//! 存档解码/解析基准（CodSpeed criterion 兼容层）。
//!
//! 覆盖 `/save` 管线的纯 CPU 段（网络拉取与 zip 解压属 IO/集成测试范畴，不在此测）：
//! - `decrypt_zip_entry`：AES-256-CBC 解密（默认存档路径，`DecryptionMeta::default()`）
//! - `derive_key`：PBKDF2-SHA1 密钥派生（旧存档路径，生产默认 1000 轮，
//!   见 client.rs 的 `kdf.rounds.unwrap_or(1000)`）
//! - `parse_game_record_bytes`：gameRecord 二进制解析（600 首，贴近真实存档规模）
//! - `parse_summary_base64`：summary 解析
//!
//! 本地运行：`cargo bench -p impl-save --bench save_decode`
//! （注意：不带 `--bench` 具名选择时，`cargo bench` 会先跑 lib 单测，
//! criterion 专属参数会被 libtest 拒收；具名选择只跑本基准。）
//! CodSpeed：`cargo codspeed run --workspace`（见 .github/workflows/codspeed.yml）

// bench 断言与构造惯例同集成测试（tests/*.rs 的文件级豁免先例；
// doc_markdown：中文注释+代码词汇混排，同 workspace 级豁免理由）。
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::doc_markdown,
    clippy::items_after_statements,
    clippy::unwrap_used,
    clippy::expect_used
)]

use std::collections::HashMap;

use base64::Engine as _;
use cbc::cipher::{BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
use criterion::{Criterion, black_box, criterion_group, criterion_main};

use impl_save::decryptor::{
    DEFAULT_IV, DEFAULT_KEY, DecryptionMeta, KdfSpec, decrypt_zip_entry, derive_key,
};
use phi_save_codec::{Difficulty, parse_game_record_bytes, parse_summary_base64};

/// 合成 gameRecord 的歌曲数（贴近真实存档：Phigros 曲目约 600+）
const SONG_COUNT: usize = 600;

// ── gameRecord 合成数据（编码格式与 phi-save-codec::game_record 的单测同构）──

/// varshort 编码（与 Reader::read_varshort 对应）
fn push_varshort(buf: &mut Vec<u8>, v: usize) {
    if v < 0x80 {
        buf.push(v as u8);
    } else {
        buf.push(((v & 0x7F) | 0x80) as u8);
        buf.push(((v >> 7) & 0xFF) as u8);
    }
}

/// 构造一首歌在 gameRecord 中的完整数据块
/// （varshort(len) + song_id + 2 个修剪字节 + payload_len + mask + fc_mask + 成绩流）
fn build_song_chunk(song_id: &str, mask: u8, fc_mask: u8, scores: &[(i32, f32)]) -> Vec<u8> {
    let mut chunk = Vec::new();
    let key_full = format!("{song_id}__");
    push_varshort(&mut chunk, key_full.len());
    chunk.extend_from_slice(key_full.as_bytes());

    let payload_len = scores.len() * 8 + 2;
    chunk.push(payload_len as u8);
    chunk.push(mask);
    chunk.push(fc_mask);

    for &(score, acc) in scores {
        chunk.extend_from_slice(&score.to_le_bytes());
        chunk.extend_from_slice(&acc.to_le_bytes());
    }
    chunk
}

/// 600 首歌的完整 gameRecord entry（含 prefix 字节）。
/// 成绩模式确定性生成：每首 1~4 个难度，约 1/8 为零分（占流但不出记录），首难度偶发 FC。
fn build_game_record_entry() -> Vec<u8> {
    let mut entry = Vec::new();
    entry.push(0u8); // prefix（解析时跳过）
    push_varshort(&mut entry, SONG_COUNT);
    for i in 0..SONG_COUNT {
        let artist = i % 89;
        let song_id = format!("Track{i:04}.Composer{artist:02}");
        let diff_count = 1 + i % 4;
        let mask: u8 = (1 << diff_count) - 1;
        let fc_mask: u8 = u8::from(i % 7 == 0);
        let scores: Vec<(i32, f32)> = (0..diff_count)
            .map(|d| {
                let zero = (i.wrapping_mul(31) + d) % 8 == 0;
                let score = if zero {
                    0
                } else {
                    880_000 + (i * 13 + d * 7) % 120_000
                };
                let acc = 85.0 + ((i + d) % 15) as f32;
                (score as i32, acc)
            })
            .collect();
        entry.extend_from_slice(&build_song_chunk(&song_id, mask, fc_mask, &scores));
    }
    entry
}

/// 模拟生产侧 ChartMap（`chart_map.get(id)` 的 HashMap 查找成本保持同构）
fn build_chart_map() -> HashMap<String, [Option<f32>; 4]> {
    (0..SONG_COUNT)
        .map(|i| {
            let artist = i % 89;
            let song_id = format!("Track{i:04}.Composer{artist:02}");
            let base = 5.0 + (i * 37 % 110) as f32 / 10.0;
            (
                song_id,
                [
                    Some(base),
                    Some(base + 1.0),
                    Some(base + 2.0),
                    Some(base + 3.0),
                ],
            )
        })
        .collect()
}

// ── 解密合成数据 ──

/// 用与解密相同的 DEFAULT_KEY/IV 加密合成明文，产出 `decrypt_zip_entry` 的合法输入
/// （含 zip entry 的 1 字节 prefix）。明文为确定性伪随机字节，避免全零明文的特例。
fn build_ciphertext(plain_len: usize) -> Vec<u8> {
    type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;
    let plain: Vec<u8> = (0..plain_len)
        .map(|i| (i.wrapping_mul(131) ^ i >> 3) as u8)
        .collect();
    let enc = Aes256CbcEnc::new(DEFAULT_KEY.as_ref().into(), DEFAULT_IV.as_ref().into());
    // encrypt_padded_b2b_mut（cbc 未开 alloc feature，用 buffer-to-buffer 原地接口；
    // 输出 buf 预留一个填充块）
    let mut ct_buf = vec![0u8; plain_len + 16];
    let ct = enc
        .encrypt_padded_b2b_mut::<Pkcs7>(&plain, &mut ct_buf)
        .expect("加密构造失败");
    let mut data = Vec::with_capacity(ct.len() + 1);
    data.push(0u8); // prefix（解密时跳过）
    data.extend_from_slice(ct);
    data
}

// ── summary 合成数据 ──

fn build_summary_base64() -> String {
    let mut bytes = Vec::new();
    bytes.push(1u8); // save_version
    bytes.extend_from_slice(&21u16.to_le_bytes()); // challenge_mode_rank
    bytes.extend_from_slice(&15.4321_f32.to_le_bytes()); // ranking_score
    bytes.push(55u8); // game_version
    let avatar = b"Introduction";
    push_varshort(&mut bytes, avatar.len());
    bytes.extend_from_slice(avatar);
    for slot in 0..12u16 {
        bytes.extend_from_slice(&slot.to_le_bytes());
    }
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

// ── 基准用例 ──

fn bench_decrypt(c: &mut Criterion) {
    let meta = DecryptionMeta::default();
    for (name, plain_len) in [
        ("aes256_cbc_decrypt_32kib", 32 * 1024),
        ("aes256_cbc_decrypt_1mib", 1024 * 1024),
    ] {
        let data = build_ciphertext(plain_len);
        // 自检：解密回原长（prefix + 明文），防止构造错误导致基准"空转"
        let out = decrypt_zip_entry(data.clone(), &meta).expect("解密自检失败");
        assert_eq!(out.len(), plain_len + 1, "解密长度自检失败");
        // iter_batched：克隆输入的分配不计入计时（生产路径无此克隆）
        c.bench_function(name, |b| {
            b.iter_batched(
                || data.clone(),
                |d| decrypt_zip_entry(d, &meta),
                criterion::BatchSize::SmallInput,
            );
        });
    }
}

fn bench_pbkdf2(c: &mut Criterion) {
    let kdf = KdfSpec::Pbkdf2Sha1 {
        salt: vec![0xAB; 16],
        rounds: 1000, // 生产默认轮数（client.rs）
        password: b"bench-password".to_vec(),
    };
    let key = derive_key(&kdf, 32).expect("派生自检失败");
    assert_eq!(key.len(), 32);
    c.bench_function("pbkdf2_sha1_derive_1000r", |b| {
        b.iter(|| derive_key(black_box(&kdf), black_box(32)));
    });
}

fn bench_game_record(c: &mut Criterion) {
    let entry = build_game_record_entry();
    let charts = build_chart_map();
    let lookup = |id: &str, diff: Difficulty| -> Option<f32> {
        let idx = match diff {
            Difficulty::EZ => 0,
            Difficulty::HD => 1,
            Difficulty::IN => 2,
            Difficulty::AT => 3,
        };
        charts.get(id).and_then(|c| c[idx])
    };
    // 自检：600 首全部入库（零分难度只丢记录不丢歌）
    let parsed = parse_game_record_bytes(black_box(&entry), lookup).expect("解析自检失败");
    assert_eq!(parsed.len(), SONG_COUNT, "解析歌曲数自检失败");
    c.bench_function("parse_game_record_600", |b| {
        b.iter(|| parse_game_record_bytes(black_box(&entry), lookup));
    });
}

fn bench_summary(c: &mut Criterion) {
    let b64 = build_summary_base64();
    let parsed = parse_summary_base64(black_box(&b64)).expect("summary 自检失败");
    assert_eq!(parsed.save_version, 1);
    c.bench_function("parse_summary_base64", |b| {
        b.iter(|| parse_summary_base64(black_box(&b64)));
    });
}

criterion_group!(
    benches,
    bench_decrypt,
    bench_pbkdf2,
    bench_game_record,
    bench_summary
);
criterion_main!(benches);
