#![allow(
    clippy::similar_names,           // 允许 in_idx 和 id_idx 同时存在
    clippy::missing_errors_doc,      // 不想为每个 Result 函数写文档
    clippy::missing_panics_doc,      // 不想为每个 .expect() 写文档
    clippy::too_many_lines,          // 允许长函数（特别是渲染逻辑）
    clippy::doc_markdown,            // 不想在注释里给每个 OpenAPI 加反引号
    clippy::struct_excessive_bools,  // 结构体里超过3个 bool 没啥大不了的
    clippy::items_after_statements,  // 允许在函数中间写 use 或 struct
    clippy::module_name_repetitions  // 允许 PlayerStats 在 player 模块里
)]

//! 本地诊断工具：拉取官方存档并输出“解密链路关键信息”。
//!
//! 安全原则：
//! - 不建议在命令行参数里传 stoken（容易被 shell history 记录），默认从环境变量读取；
//! - 默认输出为“脱敏模式”，需要显式 flag 才会输出完整 URL / 原始 summary / 明文预览。

use std::fs;
use std::fmt::Write as _;
use std::path::PathBuf;

use phi_backend::AppConfig;
use phi_backend::features::save::inspector::{
    DEFAULT_STOKEN_ENV, InspectOptions, OutputFormat, inspect_official_save,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 最小日志：仅在需要调试时启用（例如 RUST_LOG=debug）。
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    let args = Args::parse(std::env::args().skip(1).collect());
    if args.help {
        print_help();
        return Ok(());
    }

    AppConfig::init_global()?;

    let stoken_env = args.stoken_env.as_deref().unwrap_or(DEFAULT_STOKEN_ENV);
    let stoken = std::env::var(stoken_env).map_err(|_| {
        format!(
            "未找到环境变量 `{stoken_env}`（建议 PowerShell: `$env:{stoken_env}='...'`）"
        )
    })?;

    let opts = InspectOptions {
        show_full_url: args.show_url,
        show_summary_raw: args.show_summary_raw,
        preview_plain_bytes: args.preview_bytes,
        max_download_bytes: args.max_download_bytes,
    };

    let report = inspect_official_save(
        &stoken,
        &phi_backend::AppConfig::global().taptap,
        args.taptap_version.as_deref(),
        opts,
    )
    .await?;

    let output = match args.format {
        OutputFormat::Json => serde_json::to_string_pretty(&report)?,
        OutputFormat::Text => render_text(&report),
    };

    if let Some(out_path) = args.out_path {
        fs::write(&out_path, &output)?;
        println!("已写入: {}", out_path.display());
    } else {
        println!("{output}");
    }

    Ok(())
}

fn render_text(report: &phi_backend::features::save::inspector::InspectReport) -> String {
    let mut out = String::new();

    writeln!(out, "generated_at: {}", report.generated_at).expect("write inspect text");
    if let Some(v) = report.taptap_version.as_deref() {
        writeln!(out, "taptap_version: {v}").expect("write inspect text");
    }
    writeln!(out, "download_url: {}", report.meta.download_url).expect("write inspect text");
    if let Some(u) = report.meta.updated_at.as_deref() {
        writeln!(out, "updated_at: {u}").expect("write inspect text");
    }

    if let Some(s) = report.meta.summary_b64.as_deref() {
        writeln!(out, "summary_b64: {s}").expect("write inspect text");
    }
    if let Some(parsed) = report.meta.summary_parsed.as_ref() {
        writeln!(
            out,
            "summary_parsed: save_version={} challenge_mode_rank={} ranking_score={} game_version={} avatar_len={} progress=[{}]",
            parsed.save_version,
            parsed.challenge_mode_rank,
            parsed.ranking_score,
            parsed.game_version,
            parsed.avatar.len(),
            parsed.progress.iter().map(std::string::ToString::to_string).collect::<Vec<_>>().join(",")
        )
        .expect("write inspect text");
    }
    if let Some(e) = report.meta.summary_parse_error.as_deref() {
        writeln!(out, "summary_parse_error: {e}").expect("write inspect text");
    }

    writeln!(out, "download_bytes: {}", report.transport.download_bytes)
        .expect("write inspect text");
    writeln!(
        out,
        "decompress: detected={} input={} output={}",
        report.transport.decompress.detected,
        report.transport.decompress.input_bytes,
        report.transport.decompress.output_bytes
    )
    .expect("write inspect text");
    for e in &report.transport.errors {
        writeln!(out, "error: {e}").expect("write inspect text");
    }

    out.push_str("decrypt_meta:\n");
    match &report.decrypt_meta.cipher {
        phi_backend::features::save::inspector::CipherReport::Aes256CbcPkcs7 { iv_hex } => {
            writeln!(out, "  cipher: aes-256-cbc-pkcs7 iv_hex={iv_hex}")
                .expect("write inspect text");
        }
        phi_backend::features::save::inspector::CipherReport::Aes128Gcm { nonce_hex, tag_len } => {
            writeln!(out, "  cipher: aes-128-gcm nonce_hex={nonce_hex} tag_len={tag_len}")
                .expect("write inspect text");
        }
    }
    match &report.decrypt_meta.kdf {
        phi_backend::features::save::inspector::KdfReport::None => {
            out.push_str("  kdf: none\n");
        }
        phi_backend::features::save::inspector::KdfReport::Pbkdf2Sha1 {
            salt_hex,
            rounds,
            password_len,
            password_sha256_hex,
        } => {
            writeln!(
                out,
                "  kdf: pbkdf2-sha1 salt_hex={salt_hex} rounds={rounds} password_len={password_len} password_sha256={password_sha256_hex}"
            )
            .expect("write inspect text");
        }
    }
    writeln!(out, "  integrity: {}", report.decrypt_meta.integrity).expect("write inspect text");

    if let Some(z) = report.zip.as_ref() {
        writeln!(out, "zip: files={} names={}", z.file_count, z.names.join(","))
            .expect("write inspect text");
    }

    out.push_str("entries:\n");
    for ent in &report.entries {
        writeln!(
            out,
            "- {} present={} parser_handling=\"{}\"",
            ent.name,
            ent.present,
            ent.parser_handling
        )
        .expect("write inspect text");
        if let Some(len) = ent.encrypted_len {
            writeln!(
                out,
                "  encrypted_len={} encrypted_prefix={:?}",
                len,
                ent.encrypted_prefix_u8
            )
            .expect("write inspect text");
        }
        writeln!(
            out,
            "  decrypted_ok={} error={:?}",
            ent.decrypted.ok,
            ent.decrypted.error
        )
        .expect("write inspect text");
        if ent.decrypted.ok {
            writeln!(
                out,
                "  decrypted_len={:?} decrypted_prefix={:?} plain_len={:?}",
                ent.decrypted.decrypted_len,
                ent.decrypted.decrypted_prefix_u8,
                ent.decrypted.plain_len
            )
            .expect("write inspect text");
            writeln!(out, "  plain_sha256={:?}", ent.decrypted.plain_sha256_hex)
                .expect("write inspect text");
            if let Some(p) = ent.decrypted.plain_preview_hex.as_deref() {
                writeln!(out, "  plain_preview_hex={p}").expect("write inspect text");
            }
        }
    }

    if !report.notes.is_empty() {
        out.push_str("notes:\n");
        for n in &report.notes {
            writeln!(out, "- {n}").expect("write inspect text");
        }
    }

    out
}

#[derive(Debug, Clone)]
struct Args {
    help: bool,
    format: OutputFormat,
    taptap_version: Option<String>,
    stoken_env: Option<String>,
    show_url: bool,
    show_summary_raw: bool,
    preview_bytes: usize,
    max_download_bytes: usize,
    out_path: Option<PathBuf>,
}

impl Args {
    fn parse(argv: Vec<String>) -> Self {
        let mut args = Self {
            help: false,
            format: OutputFormat::Text,
            taptap_version: None,
            stoken_env: None,
            show_url: false,
            show_summary_raw: false,
            preview_bytes: 0,
            max_download_bytes: 64 * 1024 * 1024,
            out_path: None,
        };

        let mut it = argv.into_iter();
        while let Some(a) = it.next() {
            match a.as_str() {
                "-h" | "--help" => args.help = true,
                "--format" => {
                    let v = it.next().unwrap_or_else(|| "text".to_string());
                    args.format = match v.as_str() {
                        "json" => OutputFormat::Json,
                        _ => OutputFormat::Text,
                    };
                }
                "--taptap-version" => {
                    args.taptap_version = it.next();
                }
                "--stoken-env" => {
                    args.stoken_env = it.next();
                }
                "--show-url" => args.show_url = true,
                "--show-summary-raw" => args.show_summary_raw = true,
                "--preview-bytes" => {
                    if let Some(v) = it.next() {
                        args.preview_bytes = v.parse().unwrap_or(0);
                    }
                }
                "--max-download-bytes" => {
                    if let Some(v) = it.next() {
                        args.max_download_bytes = v.parse().unwrap_or(args.max_download_bytes);
                    }
                }
                "--out" => {
                    if let Some(v) = it.next() {
                        args.out_path = Some(PathBuf::from(v));
                    }
                }
                _ => {}
            }
        }
        args
    }
}

fn print_help() {
    println!(
        r"save_inspect（本地诊断工具）

用法（推荐：通过环境变量提供 stoken）：
  $env:PHI_STOKEN='...'; cargo run --bin save_inspect -- --taptap-version cn

常用参数：
  --format text|json            输出格式（默认 text）
  --taptap-version cn|global    选择 TapTap 版本（默认 cn）
  --stoken-env NAME             stoken 环境变量名（默认 PHI_STOKEN）
  --show-url                    输出完整 download_url（可能包含敏感 query）
  --show-summary-raw            输出完整 summary_b64（可能包含个人信息）
  --preview-bytes N             输出解密后明文前 N 字节（hex，默认 0）
  --max-download-bytes N        限制下载最大字节数（默认 67108864）
  --out PATH                    写入到文件（否则 stdout）
"
    );
}
