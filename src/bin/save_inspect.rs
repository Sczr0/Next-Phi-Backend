//! 本地诊断工具：拉取官方存档并输出“解密链路关键信息”。
//!
//! 安全原则：
//! - 不建议在命令行参数里传 stoken（容易被 shell history 记录），默认从环境变量读取；
//! - 默认输出为“脱敏模式”，需要显式 flag 才会输出完整 URL / 原始 summary / 明文预览。

use std::fs;
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
            "未找到环境变量 `{}`（建议 PowerShell: `$env:{}='...'`）",
            stoken_env, stoken_env
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

    out.push_str(&format!("generated_at: {}\n", report.generated_at));
    if let Some(v) = report.taptap_version.as_deref() {
        out.push_str(&format!("taptap_version: {v}\n"));
    }
    out.push_str(&format!("download_url: {}\n", report.meta.download_url));
    if let Some(u) = report.meta.updated_at.as_deref() {
        out.push_str(&format!("updated_at: {u}\n"));
    }

    if let Some(s) = report.meta.summary_b64.as_deref() {
        out.push_str(&format!("summary_b64: {s}\n"));
    }
    if let Some(parsed) = report.meta.summary_parsed.as_ref() {
        out.push_str(&format!(
            "summary_parsed: save_version={} challenge_mode_rank={} ranking_score={} game_version={} avatar_len={} progress=[{}]\n",
            parsed.save_version,
            parsed.challenge_mode_rank,
            parsed.ranking_score,
            parsed.game_version,
            parsed.avatar.len(),
            parsed.progress.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",")
        ));
    }
    if let Some(e) = report.meta.summary_parse_error.as_deref() {
        out.push_str(&format!("summary_parse_error: {e}\n"));
    }

    out.push_str(&format!(
        "download_bytes: {}\n",
        report.transport.download_bytes
    ));
    out.push_str(&format!(
        "decompress: detected={} input={} output={}\n",
        report.transport.decompress.detected,
        report.transport.decompress.input_bytes,
        report.transport.decompress.output_bytes
    ));
    for e in &report.transport.errors {
        out.push_str(&format!("error: {e}\n"));
    }

    out.push_str("decrypt_meta:\n");
    match &report.decrypt_meta.cipher {
        phi_backend::features::save::inspector::CipherReport::Aes256CbcPkcs7 { iv_hex } => {
            out.push_str(&format!("  cipher: aes-256-cbc-pkcs7 iv_hex={iv_hex}\n"));
        }
        phi_backend::features::save::inspector::CipherReport::Aes128Gcm { nonce_hex, tag_len } => {
            out.push_str(&format!(
                "  cipher: aes-128-gcm nonce_hex={nonce_hex} tag_len={tag_len}\n"
            ));
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
            out.push_str(&format!(
                "  kdf: pbkdf2-sha1 salt_hex={salt_hex} rounds={rounds} password_len={password_len} password_sha256={password_sha256_hex}\n"
            ));
        }
    }
    out.push_str(&format!("  integrity: {}\n", report.decrypt_meta.integrity));

    if let Some(z) = report.zip.as_ref() {
        out.push_str(&format!(
            "zip: files={} names={}\n",
            z.file_count,
            z.names.join(",")
        ));
    }

    out.push_str("entries:\n");
    for ent in &report.entries {
        out.push_str(&format!(
            "- {} present={} parser_handling=\"{}\"\n",
            ent.name, ent.present, ent.parser_handling
        ));
        if let Some(len) = ent.encrypted_len {
            out.push_str(&format!(
                "  encrypted_len={} encrypted_prefix={:?}\n",
                len, ent.encrypted_prefix_u8
            ));
        }
        out.push_str(&format!(
            "  decrypted_ok={} error={:?}\n",
            ent.decrypted.ok, ent.decrypted.error
        ));
        if ent.decrypted.ok {
            out.push_str(&format!(
                "  decrypted_len={:?} decrypted_prefix={:?} plain_len={:?}\n",
                ent.decrypted.decrypted_len,
                ent.decrypted.decrypted_prefix_u8,
                ent.decrypted.plain_len
            ));
            out.push_str(&format!(
                "  plain_sha256={:?}\n",
                ent.decrypted.plain_sha256_hex
            ));
            if let Some(p) = ent.decrypted.plain_preview_hex.as_deref() {
                out.push_str(&format!("  plain_preview_hex={p}\n"));
            }
        }
    }

    if !report.notes.is_empty() {
        out.push_str("notes:\n");
        for n in &report.notes {
            out.push_str(&format!("- {n}\n"));
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
                        "text" => OutputFormat::Text,
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
        r#"save_inspect（本地诊断工具）

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
"#
    );
}
