# ISSUE-0004：CI 闸门只选中根包——成员 crate 的测试代码未被 clippy/test 覆盖

- 状态：待解决
- 发现日期：2026-09
- 发现方式：接入 CodSpeed 基准测试时，本地以 `cargo clippy -p impl-save --all-targets` 验证发现 11 处 `clippy::unwrap_used` 错误，而 CI 同款命令全绿；进一步以"往 impl-save 测试代码注入类型错误"实验证实：CI 命令根本不编译成员 crate 的测试代码。
- 严重级：高（CI 绿灯给人"全仓已检查"的错觉，实为根包子集）
- 关联章节：ARCHITECTURE.md §3（crate 结构）；AGENTS.md"红线 lint / 测试"两节

## 问题陈述

根 `Cargo.toml` 是**非虚拟 workspace**（同时含 `[workspace]` 与 `[package]`）。此时 cargo 的默认包选择是**根包本身**，而不是全部成员。`build.yml` 的质量门未加 `--workspace`：

```yaml
- name: 运行 Clippy
  run: cargo clippy --all-targets --all-features -- -D warnings -W clippy::pedantic
- name: 运行测试
  run: cargo test --all-targets --all-features
```

`--all-targets` 扩展的是**目标种类**（lib/bins/tests/benches），不扩展**包选择**。因此上述两条命令实际只检查/运行 `phi-backend` 根包（含 `tests/` 金标准），而 Phase 1 拆出的 9 个成员 crate：

- **lib 代码**：作为根包依赖被 clippy 覆盖（workspace 成员经 RUSTC_WORKSPACE_WRAPPER 走 clippy-driver）✅
- **测试代码**（`#[cfg(test)]` 单测）：完全不在 CI 的编译范围内 ❌
- **`cargo test`**：成员 crate 的单测从未在 CI 运行过 ❌

AGENTS.md 推荐的本地命令 `cargo test --workspace --lib` 是带 `--workspace` 的（能覆盖成员），文档语义与 CI 实际行为不一致。

## 证据

1. 在 `impl-save/src/client.rs` 测试模块注入 `let _probe: u32 = "boom";`（类型错误）后：
   - `cargo test --all-targets --all-features --no-run`（CI 等价命令）→ **绿灯**（Finished，未编译到该代码）
   - `cargo clippy -p impl-save --all-targets` → 红灯（错误被抓到）
2. 干净 master 上 `cargo clippy -p impl-save --all-targets --all-features -- -D warnings -W clippy::pedantic` 失败：11 处 `unwrap_used`（client.rs / provider.rs 测试代码）。
3. 根 crate 与 `phi-common` 的 lib.rs 均带 `#![cfg_attr(test, allow(clippy::panic, clippy::unwrap_used, clippy::expect_used))]`，其余成员 crate 在 Phase 1 搬迁时未携带该豁免——说明测试代码本就在"被检查"的预期内。

## 影响评估

- 成员 crate 单测（如 `phi-save-codec` 的解析测试、`impl-save` 的解密测试）回归不会让 CI 变红。
- 成员测试代码的 lint 违规不可见，`-p` / `--workspace` 作用域的本地检查与 CI 行为不一致，容易误判。
- 已随 CodSpeed 接入修复的部分：`impl-save`、`impl-render` 补上 `cfg_attr(test, allow)`（与根 crate 同款）；`codspeed.yml` 的 `cargo codspeed build/run` 显式带 `--workspace`。

## 候选方案

1. **build.yml 两条命令补 `--workspace`**（最小改动）：
   - `cargo clippy --workspace --all-targets --all-features -- -D warnings -W clippy::pedantic`
   - `cargo test --workspace --all-targets --all-features`
   - 前置条件：先给其余成员 crate 补 `cfg_attr(test, allow)`（否则闸门立刻爆红）。
2. 保持现状，但把 AGENTS.md 的本地命令改为与 CI 一致的根包作用域（不推荐：掩盖缺口）。

## 验收标准

- [ ] build.yml 的 clippy/test 两条命令带 `--workspace` 且 CI 全绿
- [ ] 全部成员 crate 的 lib.rs 带 `cfg_attr(test, allow(clippy::panic, clippy::unwrap_used, clippy::expect_used))`
- [ ] 在任一成员 crate 测试代码注入类型错误，CI clippy 步骤变红（回归验证）
