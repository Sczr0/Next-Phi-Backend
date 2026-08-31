# Phi-Backend 架构总纲领（Charter）

> 版本：v1 · 状态：**冻结待执行**（作为后续所有 Phase 与变更的唯一依据）· 语言：简体中文
>
> 这是一个 **重构总纲领**，不是功能文档。回答的问题是：*如何在不打扰用户、不丢数据、可随时回滚的前提下，把现有的 Phi-Backend 内部彻底翻新。*
>
> 参考来源：以 [r0semi-mp](https://github.com/Sczr0/r0semi-mp) 的 `docs/ARCHITECTURE.md` + `AGENTS.md` 为蓝本，其元架构思想源自论文 **《A Programming Paradigm for Spatiotemporal Composability》**（cordiverse/paper。空间可组合性 = 契约 + 组合根 + 依赖方向；本项目取其契约模型，舍其运行时热替换）。

---

> ## 执行记录（2026-09，feat 分支）
>
> | 项 | 状态 | 提交 |
> |---|---|---|
> | Phase 0 基建（toolchain/deny/AGENTS/CI 六闸门/phi-common） | ✅ | f50ba5e, 7fe5c89 |
> | Phase 1 拆分（phi-contract/phi-http/impl-storage/rks/render/save/upstream + open_platform 存储并拢） | ✅ **完成** | 6626cc6…15ffb32 |
> | 唯一 sqlx（运行时代码） | ✅ 达成（dev-deps 仅为金标准集成测试） | 15ffb32, b5ed370 |
> | Phase 2 Step 1 StorageError | ✅ | d02427c |
> | Phase 2 Step 2 端口首站（行类型收口） | ✅ | b5ed370 |
> | Phase 2 Step 3 契约测试套件（fake + 真 SQLite 双验证） | ✅ | 0b79612 |
> | Phase 2 Step 4 semver 闸门（备案版） | ♻️ | 896f638 |
> | Docker 部署全套（镜像/compose/CNB/冒烟 CI/手册） | ✅ | 896f638 |
> | 遗留 D1（state.db/stats.db 拆分） | ⏳ **需 owner 决策**（双库迁移方案 + 配置 + 维护关联，行为敏感） | — |
> | 遗留 D5（save_submissions 保留策略） | ⏳ **需 owner 决策**（截断会影响 RKS 历史接口语义；方案：每用户保留最近 N 条 + 历史窗口） | — |
>
> 测试：241+ 全绿（103 root + impl-storage 28 + phi-contract 3 + 其余），每次提交 `cargo check --all-targets` = 0。

---

## 1. 我们要做什么

### 1.1 项目一句话

> **一个对外服务**（`/api/v2`、`user_hash` 身份、SQLite 业务数据）**完全不变的 Phigros 社区后端；本次变更只翻新内部**：拆分分层、抽端口、修已确诊的病灶（统计慢、数据库调用混池）、立工程纪律、补齐 Docker 部署。

### 1.2 目标（按优先级）

| 优先级 | 目标 | 含义 |
|---|---|---|
| **P0** | 对外契约零变化 | `/api/v2` 路由与 payload、`user_hash_salt`、`jwt_secret`、HTTP 语义 —— 逐字节不变（§2） |
| **P0** | 行为零回归 | 现有集成测试（`tests/api_contract_v2.rs`、`auth_contract_v2.rs`、`song_search_controls.rs`、`b27_performance_test.rs` 等）作为**金标准**，全程必须全绿（§4.4） |
| **P1** | 内部七层物理拆分 | 网关 / 业务 / 端口 / 实现 四类 crate，依赖方向由 `cargo metadata` + CI 强制（§3） |
| **P1** | 修已确诊病灶 | 统计接口过慢、单库读写混池、`daily_user` 索引方向、SQLite 文件不收缩、`save_submissions` 无限增长（§6） |
| P2 | 工程纪律 + Docker | 工具链钉死、依赖许可、CI 多道闸门、ADRs、AGENTS.md、容器化交付（§7 / §8） |

### 1.3 非目标（明确不做）

- **不重写领域算法**：存档 codec、RKS 计算、SVG/PNG 渲染、统计聚合数学 —— 已经证明正确的部分原位保留（git log 里那些 `fix(stats): 修复预聚合时区口径` 等是用 bug 换来的知识，不许重付）。
- **不引入新基础设施**：不加 PostgreSQL / Redis / 其它数据库。SQLite + moka 在 DAU 100–200 下是正确选型（选型雷达见 Annex A）。
- **不改变并发模型**：phi 是 HTTP 请求-响应，没有"每房间 actor"这类状态隔离单位；r0semi-mp 的 actor 模型**不适用**。
- **不做灰度/多活**：单人单实例，安全网 = 金标准测试 + 秒级回滚 + 短暂维护窗。

### 1.4 铁律（为什么是"装修"不是"拆房"）

1. **外部契约是公开接口，内部结构是私有实现** —— 只有前者是承诺，后者随便改。
2. **每次变更必须是"可运行的微小增量"** —— 每个 commit 后 `cargo test` 必须全绿，整条分支始终可 bisect。
3. **先搬迁、后抽象** —— 抽象等第二个实现出现（论文原则 5）。

---

## 2. 不可变对外契约（红线，动了就是事故）

| # | 契约 | 内容 | 为什么不可变 |
|---|---|---|---|
| C1 | API 面 | `/api/v2/*` 路由、请求/响应 payload（含 `Swagger` 文档语义） | 站点前端直接消费；变 = 前端联动 + 客户端全换 |
| C2 | 身份键 | `user_hash_salt`（`APP_STATS_USER_HASH_SALT`）与 `identity_hash.rs` 的 HMAC-SHA256 推导 | `user_hash` 是所有表（`events/daily_user/leaderboard_rks/user_profile/save_submissions/moderation_flags`）的连接键；**换盐 = 全体用户身份更替 = 统计孤儿化**（§5.1） |
| C3 | 会话密钥 | `session.jwt_secret`（`APP_SESSION_JWT_SECRET`）、`exchange_shared_secret`、embed secret | 相同 `jwt_secret` = 旧 access token 在新后端继续有效 = **无强制下线**（§5.2） |
| C4 | 数据 | `usage_stats.db` 中的业务数据（榜单 / 存档流水 / 资料 / 封禁 / 会话黑名单） | 用户资产；迁移原则 = 回填 + 保留 `user_hash`，不重建 |

> **结论**：C1–C4 冻结 → 用户无感、统计不断、回滚一秒。任何"重构是否安全"的争论都以本表为准。

---

## 3. 目标架构：六边形分层

### 3.1 结构化视图

```
入站 HTTP 层（网关，薄：解析/鉴权/request-id/限流）
        ↓ 只做：认证强制 + 注入请求上下文
业务特征层（第一公民）：auth / leaderboard / rks / song / stats / open_platform
        ↓ 只认识端口（trait），零 sqlx / 零 reqwest
端口层（契约）：repository 端口、存档端口、渲染端口、上游端口 + 契约测试
        ↓ 实现
impl-storage（SQLite，唯一 sqlx 所在） / impl-save / impl-render / impl-upstream
```

### 3.2 crate 结构（Phase 2 完成态）

> **Phase 1 阶段性进度（2026-09）**：已物化 `phi-common`（config 纯类型）、
> `phi-contract`（SongCandidatePreview 等对外契约类型）、`phi-http`
> （AppError/ProblemDetails/request_id 中间件——**Phase 1 临时归属**，Phase 2
> 组合根成型后并入 phi-server 网关层）。根 crate 的 `src/{config,error,request_id}.rs`
> 与 `contracts/*`、`features/*/models` 均保留 re-export shim，调用点路径不变。

```
Phi-Backend/
├── Cargo.toml                 # [workspace] + workspace.lints + workspace.dependencies
├── rust-toolchain.toml        # 钉死工具链
├── deny.toml                  # cargo-deny：依赖许可/漏洞
├── AGENTS.md                  # AI/协作者纪律（5 分钟上手）
├── docs/
│   ├── ARCHITECTURE.md        # 本文档（总纲领）
│   └── adr/                   # 架构决策记录（编号连续，check-adr.py 校验）
├── tools/check-deps.py        # 依赖方向物理闸门（CI 第 3 道）
└── crates/
    ├── phi-contract/          # 【契约】领域类型 + 端口 trait + StorageError + 契约测试套件
    ├── phi-core/              # 【柜台】业务编排：各业务服务、单生产者调度器（聚合/回收/同步）
    ├── impl-save/             # 存档：codec 解密 + 整理成领域模型（复用 phi-save-codec）
    ├── impl-render/           # 渲染：minijinja SVG + resvg PNG（重 CPU 依赖隔离在此）
    ├── impl-storage/          # 存储：全部 SQL / 池 / DDL / 索引 / VACUUM / 归档（唯一 sqlx）
    ├── impl-upstream/         # 出站：TapTap / LeanCloud / 官服 / 曲绘仓库 / GitHub
    └── phi-server/            # 【组合根】axum Router 装配 + main.rs，唯一认识所有人
```

### 3.3 依赖方向矩阵（硬约束，`check-deps.py` 强制）

| 谁 → 依赖谁 | phi-contract | phi-core | impl-* | phi-server |
|---|---|---|---|---|
| phi-contract | – | ✗ | ✗ | ✗ |
| phi-core | ✅ | – | ✗ | ✗ |
| impl-* | ✅（+ `phi-save-codec` 仅 impl-save） | ✗ | ✗ | ✗ |
| phi-server | ✅ | ✅ | ✅ | – |

四条铁律：

1. **phi-contract**：只依赖 std + `thiserror` + `serde`。零 tokio、零 sqlx、零 reqwest、零渲染。
2. **phi-core**（业务层）：只认识 phi-contract。**`sqlx` / `reqwest` / `resvg` 在依赖图中只允许出现在 `impl-*` 里** —— "业务层不直接调数据库"从口头约定变成 CI 检查。
3. **impl-\***：只认识 phi-contract（impl-save 另可依赖 `phi-save-codec`）；**impl 之间互不认识**（如 impl-storage 不得 import impl-render）。
4. **phi-server**：唯一认识所有人；组合根接线 = "老板换货架"。

### 3.4 端口（trait）规约

- **薄缝**：接口只做"形状被领域类型钉死的最小 trait"，禁止预测性丰富接口（15 个方法、能力声明、插件框架）。
- **按业务聚合拆小接口**（`LeaderboardRepo` / `StatsRepo` / `SaveRepo` / …），**禁止** `Database` 神接口（100 方法的巨型 trait 会把爆炸范围从"存储"挪回"接口"）。
- **对象安全**：所有异步 trait 一律 `#[async_trait]`（或 trait-variant），以 `Arc<dyn T>` 持有。

### 3.5 错误规约（本重构的核心红线之一）

**`StorageError`（及其它 impl 错误）必须是 phi-contract 里的纯领域枚举，严禁透传 `sqlx::Error` / `reqwest::Error`。**

```rust
// phi-contract/src/error.rs —— 纯领域错误；此文件永不 import sqlx
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("not found: {0}")]        NotFound(String),      // → 404
    #[error("duplicate: {0}")]       Duplicate(String),      // → 409
    #[error("connection failed: {0}")] ConnectionFailed(String), // → 503 类
    #[error("internal: {0}")]        Internal(String),       // → 500
}
```

- **孤儿规则陷阱（必须遵守）**：`impl From<sqlx::Error> for StorageError` **写不了**（`From` 外部 trait + `sqlx::Error` 外部类型 + `StorageError` 属于 phi-contract，impl-storage 无本地角色）；且 `thiserror` 的 `#[from]` 写在枚举定义处 → 会逼 phi-contract 依赖 sqlx。**正确形态**：转换函数放 impl-storage：

```rust
// impl-storage/src/error.rs
pub(crate) fn map_sqlx(ctx: &str, e: sqlx::Error) -> StorageError {
    tracing::warn!(ctx, error = %e, "storage error");   // 原始错误先落日志（Internal(String) 会丢来源链）
    StorageError::Internal(format!("{ctx}: {e}"))
}
```

- **变体粒度按"业务需要分支"定**：`NotFound/Duplicate/ConnectionFailed/Internal` 够用；新变体等出现真实业务分支需求再加（原则 5 同样适用于错误类型）。
- phi-core 把 `StorageError` 映射为 `AppError`（带 HTTP 状态）；**API 层永不接触 impl 错误类型**。

### 3.6 DI 规约

- **选型：`Arc<dyn T>` + `#[async_trait]`**，而非泛型。理由：接口简单、无泛型传染、编译更快；repository 是 I/O 型接口，动态派发成本可忽略。
- trait 超约束写作 `pub trait LeaderboardRepo: Send + Sync`。
- 组合根构造具体实现并 `Arc::new`；测试注入 fake（实现同一 trait）。
- 唯一可能例外：`stats.insert_events` 这类每请求热点，若真被 profile 出瓶颈，可对该**一个**接口做局部具体化 —— 不预优化。

### 3.7 网关层规约（薄）

- **入站 HTTP 层只做**：request-id 注入、CORS、限流、bearer 解析 → 请求上下文注入。
- **鉴权拆两半**：认证强制（中间件，在网关）与认证流程（TapTap QR / exchange / GitHub OAuth，属业务层 `auth` 模块）。**禁止网关持有业务流程**。

---

## 4. 执行纪律：两阶段 commit

> 总原则：**每一步变更都极小、可运行、可回滚；整条分支始终绿、始终可 bisect。**

### 4.0 Phase 0 —— 底座下沉（隐含的必需步骤）

拆分后 root 依赖 impl crates，但 `error.rs`/`config.rs` 被所有模块使用 → 必须先下沉：

1. 建 workspace 骨架（`Cargo.toml` + `rust-toolchain.toml` + `AGENTS.md` + `deny.toml`）。
2. `phi-common`（或并入 phi-core 雏形）：`AppError` / 配置结构纯类型下沉。impl crates 依赖 `phi-common`，依赖图无环。

### 4.1 Phase 1 —— 纯搬迁（不抽接口）

按依赖叶子序逐 crate 搬移，**每个 commit 后 `cargo test` 全绿**：

```
phi-common → phi-save-codec(已有) → impl-save → impl-render → impl-upstream → impl-storage → phi-backend(保留 router/features)
```

- impl-storage 先导出 **具体 struct** 给 server 拼装，**此阶段不建 trait**。
- 现有集成测试**原封不动**当金标准 → 全绿 = 证明"纯搬无回归"。
- 允许 impl 之间偶有临时依赖；`check-deps.py` **Phase 2 再启用**（否则误红）。

### 4.2 Phase 2 —— 抽端口

按领域逐个推进（leaderboard → stats → save → …）：

1. 把 `LeaderboardRepo` 等 trait + `StorageError` 沉降到 phi-contract；
2. impl-storage 实现 trait（把 SQL 从 features/* 汇入）；
3. **每个 repo trait 配契约测试套件**（fake 与真 SQLite 都通过）—— 这是"换库那天"的验证凭证；
4. phi-core 切断对具象存储的直接依赖；
5. 启用 `check-deps.py` + 依赖矩阵（§3.3），此后依赖违约 = CI 红。

### 4.3 回归网络（金标准清单）

- `tests/api_contract_v2.rs`、`tests/auth_contract_v2.rs`、`tests/song_search_controls.rs`、`tests/leaderboard_*`、`tests/b27_performance_test.rs`、`tests/bearer_auth_integration.rs`、`tests/cors_layer.rs`、`tests/request_id_integration.rs` —— 全部保留、全部必须绿。
- 每个 Phase 结束跑 `cargo fmt --check` + `cargo clippy -- -D warnings` + `cargo test`。

---

## 5. 数据迁移与连续性策略

> 参考 r0semi-mp 的"安全网 = 契约测试 + Oracle 对照 + 秒级重启"——DAU 100–200 不需要蓝绿多活。

### 5.1 身份连续性（最重要）

- **`user_hash_salt` 冻结、永不轮换**（C2）。它是假名化盐，不是签名密钥；轮换 = 一次身份事故（§2 / 此前评审记录）。
- 若未来出现泄露级事故必须轮换：走"版本化盐 + 懒重绑定"（`v1.<hex>` 前缀 + 旧新盐共存 + 用户下次登录时迁移）——**本纲领不预设该路径，出现时另立 ADR**。

### 5.2 会话连续性

- **`session.jwt_secret` 等密钥随配置原样带过去**（C3）→ 旧 access token 在新后端继续有效 → 无强制下线。
- 会话黑名单/登出闸门表（`session_token_blacklist` / `session_logout_gate`）随数据一并迁移。
- 注意：当前 `decode_access_token` 为单密钥无 `kid`，**换 `jwt_secret` 会踢掉所有在线用户** —— 未来做密钥轮换必须先进化为"kid 多钥重叠验证"（另立 ADR，不在本纲领范围）。

### 5.3 数据回填（统计连续性）

- 老库 → 新库：`events`、`daily_*`、**Parquet 归档**（保留 `user_hash` 字段不动）。
- **聚合表是可重放重建的**（DELETE+INSERT 幂等，`archive_one_day` / `stats_archive_reconcile` 已是现成重放路径）→ 即便原始 `events` 已被保留期清理，历史 DAU/Summary 仍可重建。

### 5.4 换挡与回滚

1. 并行开发：新分支慢慢写，master 继续对外服务。
2. 影子校验：新后端读老库拷贝、接真实流量比对（`daily_*` 数值与旧后端一致 + 抽样 user_id 解析一致）。
3. 短维护窗换挡（凌晨 2–3 分钟冻写 → 最终迁移 → 校验 → 指向新后端）。
4. **回滚预置**：旧二进制 + 迁移前快照常驻；单 SQLite 文件 + 单二进制 → 回滚 = 几秒。

---

## 6. 已确诊病灶清单（重构必须修复）

| # | 病灶 | 症状 | 修复 | 阶段 |
|---|---|---|---|---|
| D1 | 单库读写混池 | `events` 高频 append + 后台聚合/归档与 API 读写共用 `usage_stats.db` 一个池；`busy_timeout=5s` 下请求可能被后台重写拖住数秒 | 拆 `state.db`（榜单/存档/资料/封禁/会话）与 `stats.db`（events/daily_*/归档），独立池 | Phase 2 后 |
| D2 | DAU 索引方向反 | `idx_daily_user_hash ON daily_user(user_hash)` 是 user-leading；`summary.rs` 按 date 区间 `DISTINCT user_hash` 走不了该索引 | 加 `(date, user_hash)` 复合索引；`daily_ip` 同理 `(date, client_ip_hash)` | Phase 1 前（独立小 PR） |
| D3 | 聚合触碰请求路径 | 部分聚合/热窗口重算可能请求时触发，大范围时卡 | 聚合/归档/热窗口重算完全移出请求路径，由 phi-core 单生产者调度器执行 | Phase 2 |
| D4 | SQLite 文件不收缩 | 清理后只 `wal_checkpoint(TRUNCATE)`（缩 WAL 不缩主库）；无 `VACUUM`/`auto_vacuum` | `PRAGMA auto_vacuum=incremental`（建库时）+ 定期 `VACUUM` | Phase 1 前（小 PR） |
| D5 | `save_submissions` 无限增长 | 非时间序列、未归档，随玩家数持续增长 | 按 user 封顶（保留每人最近 N 条）或明确保留策略（另立 ADR 定 N） | Phase 2 后 |
| D6 | summary 回退扫 `events` | 部分查询（`unique_ips` 等）回退到原始表 `COUNT(DISTINCT)`，最贵的单点查询 | 保证聚合表完整 + 正确索引，杜绝回退（**2026-09 注**：本次已落地索引加速（D2）且聚合完整性由 backfill 哨兵+每日自愈保证；"杜绝回退"暂缓——feature 维度查询无法从 daily_* 服务（无该列），完整消除需 Phase 2 增列，回退路径本身已有 `idx_events_ts_user/ip` 索引支撑） | Phase 1 前（索引部分已随 D2 落地） |
| D7 | 无 VACUUM/优化 | 数据库长期运行无维护 | 定期 `VACUUM` + `PRAGMA optimize` 纳入调度器 | 同上 |

> 原则：D2/D4/D6/D7 是**低风险高收益小 PR**，应先行（不依赖重构）；D1/D3/D5 依赖分层落地。

---

## 7. 工程质量纪律

### 7.1 工具链

- `rust-toolchain.toml` **钉死版本**（channel + components：rustfmt/clippy + profile=minimal）——可复现构建（参考 r0semi-mp 的 1.98.0）。

### 7.2 第三方依赖

- `deny.toml`：license 白名单（Apache-2.0/MIT/BSD-3-Clause/ISC/Zlib/MPL-2.0 等）+ `[advisories] yanked = "deny"`（RUSTSEC 忽略项必须附带理由）+ `[sources] allow-git` 白名单。
- CI 用 cargo-deny-action；新增依赖必须落在白名单内。

### 7.3 CI 六道闸门（对齐 r0semi-mp）— 落地时间：Phase 0

```
1. cargo fmt --all -- --check
2. cargo clippy --workspace --all-targets -- -D warnings
3. python3 tools/check-deps.py          # Phase 2 启用
3b. python3 tools/check-adr.py          # ADR 编号连续
4. cargo test --workspace --all-targets
5. cargo deny check                      # 许可/漏洞
6. cargo-semver-checks                   # phi-contract 破坏性变更检测（Phase 2 启用）
```

### 7.4 Lint 红线

- `unsafe_code = "forbid"`（全 workspace）。
- phi-contract：`missing_docs = "deny"`（契约必须文档化）。
- 全 workspace clippy `all + pedantic -D warnings`（保留现有 `panic/unwrap_used/expect_used` 红线）。
- phi-core 禁 unwrap/expect（业务层不许 panic）。

### 7.5 测试分层

| 层 | 内容 | 位置 |
|---|---|---|
| 金标准 | 现有全部集成测试（行为回归网） | `tests/` |
| 契约测试 | repo/端口 trait 的泛型套件，fake 与真实现都必须过 | `phi-contract/src/suite.rs` |
| 单元测试 | 领域算法（RKS/渲染/聚合数学）原地保留 | 各 module |
| 深度验证（可选，后期） | mutants / llvm-cov / fuzz（解码 target） | 独立 workflow，不进主闸门 |

### 7.6 文档体系

- **`docs/adr/NNNN-*.md`**：架构决策记录，编号连续（`tools/check-adr.py` 校验）。
- **`docs/issues/`**：文档说了一套、代码做了另一套时，用 issue 文档形式记录（状态/发现日期/严重级/关联章节）。
- **`AGENTS.md`**：给 AI 协作者/新人的 5 分钟纪律手册（什么能信、什么不能信、怎么改不出错）。

---

## 8. 部署

### 8.1 容器化（Phase 0 后逐步落地）

- **多阶段 Dockerfile**：builder（clux/muslrust 或 dtolnay 装 target）→ runtime（alpine）单文件静态二进制（`--profile release-dist`，strip+fat LTO+codegen-units=1）。
- **运行时**：`HEALTHCHECK`（已有 `/health`）、`STOPSIGNAL SIGTERM`（已有 `shutdown.rs` 优雅停机 + `watchdog.rs` sd-notify 能力充分利用）。
- **挂载**：`./data`（`state.db`/`stats.db`）、`./info`、`./resources/ill`（曲绘）；密钥全部走 `APP_*` 环境变量（代码已支持）。
- **docker-compose.yml**：healthcheck + volume + restart + env 覆盖 + `stop_signal: SIGTERM`。
- **可选 .cnb.yml**：CNB 云原生构建推国内 Docker 制品库（r0semi-mp 已验证该路径，适合国内拉取）。

### 8.2 配置原则

- 配置文件只含非敏感项；所有密钥（`jwt_secret`/`user_hash_salt`/`exchange_shared_secret`/`merkle_salt`/`watermark.*` 等）只经环境变量注入 —— **敏感值不进 git**（当前 `config.example.toml` 已有此规范，重构时保持）。

---

## 9. 参考与致谢

- [r0semi-mp](https://github.com/Sczr0/r0semi-mp)：依赖方向矩阵、契约测试、两阶段纪律、CI 六道闸门、Docker/CNB 路径的直接蓝本。
- 《A Programming Paradigm for Spatiotemporal Composability》（cordiverse/paper）：空间可组合性（契约 + 组合根 + 依赖方向）的形式化基础；时间可组合性以 RAII + 重启替代。
- 原版 [phira-mp](https://github.com/TeamFlos/phira-mp) / Phigros 社区生态：协议与业务事实的权威参考。

---

## Annex A：选型雷达（为什么新增数据库是伪需求）

| 技术 | 何时才该用 | 本项目 |
|---|---|---|
| SQLite | 单实例、读写均衡、零运维、秒级回滚 | ✅ 就该用它 |
| PostgreSQL | 多实例共享、复杂联结/并发写、JSONB/全文 | ❌ 什么都不沾 |
| Redis | 多实例共享缓存/会话/限流、Pub/Sub | ❌ 单实例，moka 已覆盖（进程内） |
| moka | 进程内缓存，单实例最省 | ✅ 已在用 |
| DuckDB | 对 Parquet/CSV 做分析查询（冷历史统计） | ⚠️ 可选加分：读 `events-*.parquet` 归档；非主库 |

## Annex B：术语表

| 词 | 含义 |
|---|---|
| 契约（contract） | 对外承诺的接口（HTTP 路由 / 端口 trait / 错误枚举），是"不可变、可测试"的东西 |
| 端口（port） | 业务层依赖的抽象接口（trait），实现（adapter）可替换 |
| 组合根（composition root） | 唯一认识所有 crate 的入口（`phi-server/main.rs`），负责接线 |
| 金标准测试 | 现有集成测试集合，用作行为回归的对照组 |
| 单生产者调度器 | 唯一负责"时间/周期期界"（聚合、归档、资源同步）的后台任务；事实必须命令化、按序派发 |
