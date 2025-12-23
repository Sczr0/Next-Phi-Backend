# save_inspect（本地存档解密链路诊断工具）

日期：2025-12-21  
执行者：Codex

## 目的

在不泄露 `stoken` 的前提下，拉取官方存档并输出“解密链路关键诊断信息”，用于判断：

- `crypto` 元信息是否发生变更（模式/KDF 等）
- zip entry 的第 1 字节（prefix）在解密与解析阶段如何被处理
- 哪些 entry 缺失/解密失败（以及失败原因）
- `summary`（LeanCloud `_GameSave.summary`）的原始值与解析结果

## 安全说明

- 强烈建议通过环境变量提供 `stoken`，避免命令行历史记录泄露。
- 默认输出为“脱敏模式”：不输出完整 download_url、不输出完整 summary_b64、不输出明文预览；需要显式开启参数。

## 使用方法（PowerShell）

```powershell
$env:PHI_STOKEN = "your-session-token"
cargo run --bin save_inspect -- --taptap-version cn
```

输出 JSON（便于粘贴/对比）：

```powershell
cargo run --bin save_inspect -- --format json --taptap-version cn
```

输出原始 summary（可能包含个人信息）：

```powershell
cargo run --bin save_inspect -- --show-summary-raw --format json
```

输出明文预览（hex，谨慎使用）：

```powershell
cargo run --bin save_inspect -- --preview-bytes 32 --format json
```

## 输出字段说明（简要）

- `meta.summary_b64`：原始 summary（默认脱敏；`--show-summary-raw` 可输出完整）
- `meta.summary_parsed`：summary 结构化解析结果（失败时见 `summary_parse_error`）
- `entries[*].encrypted_prefix_u8`：zip entry 第 1 字节（解密前）
- `entries[*].decrypted.decrypted_prefix_u8`：解密后输出的第 1 字节（通常与 encrypted_prefix 一致）
- `entries[*].parser_handling`：对应 `parser.rs` 对 prefix 的处理方式（跳过 / 作为 version）
- `entries[*].decrypted.plain_sha256_hex`：明文（去除 prefix）的哈希，用于对比一致性（不泄露正文）

