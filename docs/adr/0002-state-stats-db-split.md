# ADR-0002：state.db / stats.db 拆分方案（D1——待 owner 决策后实施）

- 日期：2026-09
- 状态：草案（方案已定，**待 owner 拍板后实施**）
- 相关章节：docs/ARCHITECTURE.md §6（D1）、§5（迁移流程）

## 背景

当前所有业务表共用一个 `resources/usage_stats.db`：接口热路径（榜单/资料/封禁/会话
黑名单）、高频埋点 append（`events`）、后台聚合/归档（DELETE+INSERT + VACUUM）
共用同一池。已观察到的病症：后台重写任务与 API 读抢池、`busy_timeout=5s` 下
请求可能被拖住数秒（§6 D1）。

## 方案（推荐）

**一库两文件，物理隔离写入路径：**

| 库 | 归属表 | 特征 |
|---|---|---|
| `usage_stats.db` → 改名 `stats.db` | `events`、`daily_*`、`stats_meta`、Parquet 归档路径 | append 高频 + 批量聚合（**保留现状**，池建议 `max_connections=8`） |
| 新增 `state.db` | `leaderboard_rks`、`leaderboard_details`、`user_profile`、`save_submissions`、`user_moderation_state`、`moderation_flags`、`session_token_blacklist`、`session_logout_gate`、`developers`、`api_keys`、`api_key_events` | 业务热路径（读多写少、点查为主，池 `max_connections=4`） |

**接线**：`StatsStorage` 保持为"stats 库"入口（现有调用点不变）；新增
`StateStorage`（或 `StatsStorage::state_pool()` 第二池）承载业务表读写；
各 handler 经 `state.stats_storage` 的现有引用平移（组合根装配两个池）。
impl-storage 内按表归属迁移 DDL 与查询方法——**这是一个纯内部结构变化，对外零变化**。

**一次性迁移（维护窗，见 §5 流程）**：
1. 停写（维护窗）；
2. `usage_stats.db` 拷贝为 `state.db`，删掉 `state.db` 中的统计表；
3. `state.db` 复制回删掉业务表的 `usage_stats.db`（或反向——以拷贝+kill 业务表为准，
   确保两边表集互斥）；
4. `VACUUM` 两库；`PRAGMA integrity_check`；
5. 启动新版本（两个连接池），验证 `/health` + 抽样榜单/统计查询；
6. 回滚 = 恢复原库快照 + 旧二进制（秒级）。

**配置**（新增，均有默认值）：`stats.sqlite_path`（默认 `./resources/usage_stats.db`、
语义不变）、`stats.state_db_path`（新增，默认 `./resources/state.db`）。
密钥不变（C2/C3 零影响）。

## 后果

- 正面：热路径与埋点/聚合互不阻塞（D1 根治）；聚合/归档/VACUUM 的锁只影响 stats 池。
- 负面：一个迁移维护窗（预计 <5min，DAU 100-200 下无感）；两库备份（备份脚本拷贝两个文件）。
- 风险与对策：迁移脚本必须**断言表集互斥**（两边各含预期表集，防漏删/多删）；
  迁移前自动拷贝快照到同目录 `.bak`；回滚保留旧二进制 + 快照。

## 待 owner 确认

1. 按本方案拆，还是等 Phase 3（handler 切端口后）再拆？（**推荐**：随 Phase 2
   端口收口后拆——表归属与读取方法已按域归位，拆库=只动池装配，风险最小）
2. 迁移窗口与备份目录约定（建议 `resources/_bak_<日期>/`）。
