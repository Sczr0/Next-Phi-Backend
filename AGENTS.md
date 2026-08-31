# AGENTS.md — Phi-Backend 项目纪律（给 AI Agent / 协作者）

> 本文是**工作纪律摘要**，不是架构文档。架构细则见 [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)（总纲领，权威）。
> 目标：让"每次会话都是新协作者"的 AI 在 5 分钟内掌握：什么能信、什么不能碰、怎么改不出错。

## 项目一句话

Phigros 社区后端（Rust 2024 + axum + SQLite），提供存档解析、B27 成绩图渲染、RKS 榜、统计与开放平台 API、会话认证。当前处于**内部重构期**（总纲领 v1，feat 分支），对外服务必须保持逐字节一致。

## 不可变对外契约（红线，动了就是事故）

| # | 契约 | 内容 |
|---|---|---|
| C1 | API 面 | `/api/v2/*` 路由 + 请求/响应 payload + Swagger 语义 |
| C2 | 身份键 | `user_hash_salt`（`APP_STATS_USER_HASH_SALT`）与 `identity_hash.rs` 的 HMAC-SHA256 推导；**换盐 = 全体用户身份更替 = 统计孤儿化（灾难）** |
| C3 | 会话密钥 | `session.jwt_secret` 等；**换 = 全体在线用户被踢** |
| C4 | 数据 | `usage_stats.db` 中的业务数据（榜单/存档流水/资料/封禁/会话黑名单） |

**改代码前先自问：这会不会让 C1-C4 变？会 -> 先立 ADR 再动手。**

## 文档可信度

- `docs/ARCHITECTURE.md`（总纲领）：✅ 以它为准；含已确诊病灶 D1-D7 清单。
- 代码注释：✅ 大量注释记录真实决策（如归档"先归档后清理"的自愈守卫）。
- **以代码为准**：发现"文档说了、代码没做" -> 记入 `docs/issues/ISSUE-00XX-*.md`（格式见下），不得假装已解决。

## 修改纪律

1. **依赖方向（未来强约束）**：目标 crate 结构：`phi-contract`（契约，零 tokio/零 sqlx）→ `phi-core`（业务编排）→ `impl-*`（实现）→ `phi-server`（组合根）。当前仍为单 crate，**新增 crate 前先读总纲领 §3，并向主人确认**。
2. **错误不得透传**：impl 错误（`StorageError` 等）必须是领域枚举（`NotFound/Duplicate/ConnectionFailed/Internal`），禁止把 `sqlx::Error` 直接冒出 API。**孤儿规则**：`#[from] sqlx::Error` 只能写在 enum 定义处，绝不允许把 sqlx 依赖拉进契约 crate；转换函数放 impl 侧（`map_sqlx`）。
3. **DI**：`Arc<dyn T>` + `#[async_trait]`，trait 超约束 `Send + Sync`；禁止泛型传染。
4. **两阶段**：先搬迁（纯移动、行为不变、测试全绿）后抽象（抽 trait）。**禁止**在同一提交里既搬代码又改行为。
5. **性能病灶（已确诊）**：统计慢的根因是索引/池/聚合位置（D1-D7），不是"换数据库"。**禁止**为提性能引入 PostgreSQL/Redis（选型雷达见总纲领 Annex A）——SQLite + moka 在 DAU 100-200 正确。
6. **调度纪律**：时间/周期事实（聚合、归档、资源同步）统一由单生产者调度器派发，禁止各模块自开 ad-hoc 线程。

## 红线 lint（当前生效）

- 生产代码 `panic!` / 无上下文 `unwrap` / `expect`：`warn`（测试经 `cfg_attr(test, allow)` 豁免）。
- CI 以 `clippy -D warnings -W clippy::pedantic` 卡死——提交前先本地过。
- `cargo clippy --all-targets --all-features -- -D warnings -W clippy::pedantic`
- 目标（Phase 后）：全 workspace `unsafe_code = forbid`；phi-contract `missing_docs = deny`。

## 测试

```bash
cargo test --workspace --lib  # 全部单测（各 crate 分别聚合统计）
cargo check --all-targets     # 编译全目标（不链接，绕开本机 IDE 文件锁）
cargo fmt --all -- --check    # 格式化
```

- **金标准**：`tests/api_contract_v2.rs`、`auth_contract_v2.rs`、`song_search_controls.rs`、`leaderboard_*`、`b27_performance_test.rs` 等**不许改**（行为回归网）。
- **契约测试**（Phase 2 起，权威）：`phi-contract::repo::leaderboard_repo_contract_suite`——**任何实现（fake / SQLite / 未来替换）必须通过**；新增实现 crate 时同步跑同一套件。
- **错误规约**：impl 层新代码一律 `StorageError`（NotFound/Duplicate/ConnectionFailed/Internal）+ `map_sqlx` 转换；禁止把 sqlx/reqwest 错误直接冒出（孤儿规则见 phi-contract/src/error.rs）。
- 新功能必须带测试；存储/统计改动优先挂临时库测试（见 `connection.rs` 的 `vacuum_and_optimize_runs_on_temp_db` 与 `submission.rs` 的 D5 测试模式）。
- **D5 保留策略**：`trim_save_submissions_per_user` 是 opt-in（配置 >0 才清理，默认关闭）；改它之前先读 ADR-0003。

## 常用命令

```bash
cargo check --all-targets          # 本地编译验证（最快）
cargo test --lib                   # 单元测试
cargo fmt --all -- --check         # 格式
python tools/check-deps.py         # 依赖方向（当前单 crate，仍应通过）
python tools/check-adr.py          # ADR 编号连续
```

## 文档体系

- 新架构决策 → `docs/adr/NNNN-<kebab>.md`（编号连续，`python tools/check-adr.py` 校验）。
- 文档与代码不一致 → `docs/issues/ISSUE-00XX-<kebab>.md`：

```
# ISSUE-00XX：<标题>
- 状态：待解决
- 发现日期：YYYY-MM
- 发现方式：<如何发现>
- 严重级：低 / 中 / 高
- 关联章节：ARCHITECTURE.md §X
## 问题陈述 / ## 证据 / ## 影响评估 / ## 候选方案 / ## 验收标准
```

## 血泪史（改这些文件前必读）

- `identity_hash.rs` / `features/auth/`：**盐与密钥永不轮换**（C2/C3；轮换方案见总纲领 §5.1）。
- `stats/storage/daily.rs`：`DELETE+INSERT` 幂等聚合——**不许**改成 `REPLACE INTO`（NULL 主键不强制唯一会重复累加）；`repair_daily_agg_duplicates_once` 是历史重复行的修复路径。
- `stats/storage/summary.rs`：快路径以 `backfill_complete` 哨兵 + 逐日覆盖为前提；feature 维度查询走慢路径（有索引支撑），**不许**为了"快"删掉慢路径哨兵。
- `stats/archive.rs`：**先归档、后清理**（`cleanup_skips_unarchived_days` 守卫防丢数据）；清理 `CLEANUP_DELETE_BATCH_SIZE=5000` 防长事务锁写。
- `config.rs`：所有密钥默认从 `APP_*` 环境变量读取；**敏感值不得进 git**。
