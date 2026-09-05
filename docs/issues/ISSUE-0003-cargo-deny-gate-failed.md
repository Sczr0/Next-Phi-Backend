# ISSUE-0003：CI 第 5 道闸门（cargo-deny）失败——症状定位与待确认根因

- 状态：**已解决（CI 待确认）**（2026-09-05：根因确认、修复实施，本机四道验证全绿，见"处理记录"；CI quality-gate 与 `build-release` 恢复触发待推送后回写）
- 发现日期：2026-09
- 发现方式：CI 观察（`build.yml` quality-gate「第三方依赖许可/漏洞审查（Charter §7.2）」步骤红色失败；日志停在 hashbrown 重复版本警告处）
- 严重级：中（闸门阻塞 CI 合并，不影响运行时与对外契约 C1-C4）
- 关联章节：ARCHITECTURE.md §7.2；ADR-0001

## 问题陈述

cargo-deny 步骤失败（错误退出码，运行 1-5 分钟），但用户所见日志片段**无任何 `error[...]` 行**：
日志以 `[bans]`（`multiple-versions = "warn"`）的重复版本警告收尾，随后无汇总行、无诊断输出即退出非零。

## 证据（2026-09，worktree clean @ a617155，本地离线核实）

1. **重复版本警告不是失败原因**：`deny.toml` 中 `[bans] multiple-versions = "warn"`——hashbrown ×3（0.14.5/0.16.1/0.17.1）、rand ×3（0.8.7/0.9.5/0.10.2）等仅告警不使退出码非零。
2. **licenses 检查据本地缓存核实可通过**：全树 582 个包、全部 license 表达式（含 `unicode-*` 的 `(MIT OR Apache-2.0) AND Unicode-3.0` 组合、`MIT/Apache-2.0` slash 变体、`Unlicense/MIT` 等）均在 `[licenses] allow` 覆盖内。
3. **sources 检查**：`[sources]` 为空配置，默认放行。
4. **剩余唯一可能**：`advisories` 检查（`yanked = "deny"` + RUSTSEC 公告）或运行中断（OOM / advisory-db 拉取失败）；其 `error[advisories]` / `error[yanked]` 输出不在已见日志片段中。
5. 锁文件快照中值得关注的版本（2025-2026 公告雷达）：`zip 2.4.2`（RUSTSEC-2025-0168 #可疑）、`webp 0.3.1`（GHSA-9q78-27f3-2jmh）、`image 0.25.10`、`exr 1.74.2`、`tiff 0.11.3`、`git2 0.20.4`、`openssl-sys 0.9.117`；`ttf-parser` 不在树内（RUSTSEC-2026-0192 不适用）；`wasi 0.11.1`（非 yanked 版本）。

## 影响评估

- master/feat 分支无法过 quality-gate → `build-release` 不触发，发布流停摆。
- 若最终为 RUSTSEC/yanked：属**真实安全债**，应升级而非豁免。

## 候选方案

1. **定位**（先做）：取完整步骤日志（搜 `advisories`/`yanked`/`RUSTSEC`/`panic`），或本机 `cargo install cargo-deny && cargo deny check` 复现（本机有网，可直出 `error[...]` 行）。
2. 若 `error[yanked]`：`cargo update -p <crate>`（注：Cargo.lock 自 0b79612 后 7 个 commit 未动，先整体 `cargo update` 冲刷，保持 semver 不动 C1-C4）。
3. 若 `error[advisories]`：升级对应 crate（候选：`zip` → ≥2.4.2 修复版；`webp`/`image` 系列）。
4. 若 advisory-db 拉取失败/超时：cargo-deny-action 启用 `rustsec-advisory-db-cache`（或配置镜像 `db-url`）；**禁止**降级为跳过 advisories 检查。

## 验收标准

1. `cargo deny check` 全绿（或违规项按规则显式登记理由，不允许裸豁免）；
2. CI quality-gate 通过，`build-release` 恢复触发。

## 处理记录（2026-09）

本机复现 `cargo deny check` 确认根因（汇总行：`advisories FAILED, bans ok, licenses ok, sources ok`），共 6 条 RUSTSEC + 1 个 yanked：

| 项 | 修复 |
|---|---|
| RUSTSEC-2026-0183 / 0184（git2 `Remote::list` / `BlameHunk` unsound） | `git2 0.20.4 → 0.21.0`（manifest + lock）；0.21 默认 features 为空，代码经 HTTPS 拉取 illustrations 仓库（`src/startup/checks.rs`），故显式加 `https` feature（`vendored-openssl` 保留） |
| RUSTSEC-2026-0253（lru `pop` 非 panic 安全） | `lru 0.16.4 → 0.18.3`（root + impl-render manifest；代码已用 `NonZeroUsize`，API 兼容） |
| `error[yanked]` chacha20 0.10.1 | `cargo update -p chacha20` → 0.10.2 |
| RUSTSEC-2024-0436（paste 无维护，1.0.15 为末版） | 豁免（deny.toml `[advisories] ignore`，附理由：经 pulp→exr/rav1e 传递，无可用升级） |
| RUSTSEC-2026-0194 / 0195（quick-xml，仅经 pprof[flamegraph]→inferno 的 dev 依赖） | 豁免（附理由：pprof 0.14.1 为最新版，dev 依赖不进 release 产物） |

### 随附变更（同批提交，非安全修复）

| 项 | 说明 |
|---|---|
| `zip 2.4.2 → 3.0.0`（root / impl-save / phi-http） | 大版本前滚刷新，消除本 ISSUE 证据节的"可疑"标记：RUSTSEC-2025-0168 仅影响 `ZipArchive::extract`（受影响 `< 2.3.0`），2.4.2 已在修复版本区间，且本项目代码仅用 `ZipArchive::new` + `ZipError` 转换（`impl-save/src/provider.rs`、`phi-http/src/error.rs`），不经 extract 路径 |
| `gethostname 0.4 → 0.5`、`tiny-skia 0.11 → 0.12`（feature `png` 改名 `png-format`）、`png 0.17 → 0.18` | 随附依赖刷新（无 RUSTSEC 关联）；用法面窄，零 `.rs` 改动全量编译通过 |
| license `AGPL-3.0 → AGPL-3.0-only`（全部 10 个 manifest） | SPDX 规范化（`AGPL-3.0` 为废弃写法）；deny.toml 白名单本已同时含两种写法，licenses 检查此前即通过，非修复必需 |

变更文件：`Cargo.toml`（git2/lru/zip/gethostname/tiny-skia/png + license）、9 个子 crate manifest（license；impl-render 另有 lru、impl-save/phi-http 另有 zip）、`deny.toml`（3 条豁免）、`Cargo.lock`（git2 0.21.0 / lru 0.18.3 / chacha20 0.10.2 / zip 3.0.0 / libgit2-sys 0.18.8+1.9.7 等）。

### 本机验证记录（2026-09-05，修复后）

- `cargo check --all-targets`：通过
- `cargo deny check`：`advisories ok, bans ok, licenses ok, sources ok`
- `cargo test --workspace --lib`：260 个测试全过（phi-backend 103 / impl-rks 46 / impl-render 39 / impl-storage 30 / phi-save-codec 15 / impl-save 13 / impl-upstream 7 / phi-http 4 / phi-contract 3）
- `cargo clippy --all-targets --all-features -- -D warnings -W clippy::pedantic`（CI 同款）：0 警告

遗留：CI quality-gate 通过与 `build-release` 恢复触发，待推送后在 CI 确认（届时本状态行改回"已解决"）。
