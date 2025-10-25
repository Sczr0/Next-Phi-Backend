# Phi Backend — 排行榜与公开资料设计（无图片版）

作者：Codex（AI）  日期：2025-10-25

本文为内部设计文档，定义排行榜、公开资料、反作弊与管理员方案，不包含任何图片/快照内容。统一以本文为准。

## 1. 目标与范围

- 目标
  - 提供“内部可用、可对外公开”的玩家排行榜（按 RKS 排序）。
  - 支持玩家自愿公开资料：匿名别名、是否展示 RKS 构成（Best27+APTop3）与 BestTop3+APTop3（纯文字）。
  - 提供基础反作弊（可疑评分与影子隐藏）与管理员管理能力（审核/封禁/别名仲裁）。
- 范围
  - 数据来源：玩家发起的 `/save`（或等价认证）的最新成绩。
  - 存储：复用现有 `SQLite + sqlx`（同 `usage_stats.db`）。
  - 展示：REST API（纯 JSON 文本）。
- 非目标
  - 图片生成/快照持久化。
  - 完整社交系统（关注、评论、消息）。
  - 复杂反作弊对抗（模型/设备指纹攻防）。

## 2. 排序指标与身份

- 排序指标：默认使用玩家总 `RKS = Best27 + AP3` 的平均值（复用 `src/features/rks/engine.rs`）。
- 替代指标（可选）：`summary.ranking_score`（可一并存储，后续支持切换或并行榜）。
- 身份
  - 使用 `user_hash = HMAC-SHA256(salt, stable-identifier)`（统计模块已实现）。
  - `salt` 来自 `config.stats.user_hash_salt`；未配置则禁用排行榜写入（只读）。

## 3. 架构概览

```
客户端 -> /save                        -> 解析存档 + 计算 RKS
        -> leaderboard ingestor         -> 反作弊可疑评分 -> UPSERT 榜单 & 文字详情

查询端 -> /leaderboard/rks/top|me       -> 读取榜单与资料（过滤公开/隐藏）
公开端 -> /public/profile/{alias}       -> 公开资料（可含文字版 RKS 构成与 Top3）

管理端 -> /admin/leaderboard/*          -> 审核/封禁/别名仲裁
```

复用：
- 统计与连接池：`src/features/stats/storage.rs` 中的 SQLite 与初始化流程。
- RKS 计算：`src/features/rks/engine.rs`。

## 4. 数据模型（DDL）

所有表与索引在 `StatsStorage::init_schema()` 一并创建，保持单点初始化与幂等。

```sql
-- 排行榜主表（每用户一行，保存最佳成绩）
CREATE TABLE IF NOT EXISTS leaderboard_rks (
  user_hash       TEXT PRIMARY KEY,
  total_rks       REAL NOT NULL,
  user_kind       TEXT,
  suspicion_score REAL NOT NULL DEFAULT 0.0,
  is_hidden       INTEGER NOT NULL DEFAULT 0, -- 1=true（影子隐藏，不出现在公开榜）
  created_at      TEXT NOT NULL,              -- 首次上榜（UTC RFC3339）
  updated_at      TEXT NOT NULL               -- 最近更新（UTC RFC3339）
);
-- 稳定排序索引：同分按更新时间、再按 user_hash 稳定
CREATE INDEX IF NOT EXISTS idx_lb_rks_order ON leaderboard_rks(total_rks DESC, updated_at ASC, user_hash ASC);

-- 用户公开资料与别名
CREATE TABLE IF NOT EXISTS user_profile (
  user_hash   TEXT PRIMARY KEY,
  alias       TEXT UNIQUE COLLATE NOCASE,     -- 大小写不敏感唯一
  is_public   INTEGER NOT NULL DEFAULT 0,
  -- 成绩展示权限（纯文字）
  show_rks_composition   INTEGER NOT NULL DEFAULT 1,  -- 是否展示 RKS 构成信息（Best27+APTop3）
  show_best_top3         INTEGER NOT NULL DEFAULT 1,  -- 是否展示 BestTop3
  show_ap_top3           INTEGER NOT NULL DEFAULT 1,  -- 是否展示 APTop3
  user_kind   TEXT,
  created_at  TEXT NOT NULL,
  updated_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_profile_public ON user_profile(is_public);

-- 成绩上报历史（用于反作弊与审计）
CREATE TABLE IF NOT EXISTS save_submissions (
  id             INTEGER PRIMARY KEY AUTOINCREMENT,
  user_hash      TEXT NOT NULL,
  total_rks      REAL NOT NULL,
  acc_stats      TEXT,  -- 序列化的 ACC 概览/AP 比例等
  rks_jump       REAL,  -- 与上一次的跳变绝对值
  route          TEXT,  -- 触发来源（/save 或其他）
  client_ip_hash TEXT,
  details_json   TEXT,  -- 可选：具体分布与异常字段
  suspicion_score REAL NOT NULL DEFAULT 0.0,
  created_at     TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_submissions_user ON save_submissions(user_hash, created_at DESC);

-- 文字详情缓存（可选）：便于快速返回 Top3 与 RKS 构成
CREATE TABLE IF NOT EXISTS leaderboard_details (
  user_hash              TEXT PRIMARY KEY,
  rks_composition_json   TEXT, -- 包含 Best27/ApTop3 的概要（纯文字/数值 JSON）
  best_top3_json         TEXT, -- Top3 列表（曲名/难度/acc/rks）
  ap_top3_json           TEXT, -- AP Top3 列表
  updated_at             TEXT NOT NULL
);

-- 管理端标注/审核
CREATE TABLE IF NOT EXISTS moderation_flags (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  user_hash   TEXT NOT NULL,
  status      TEXT NOT NULL,       -- pending|approved|rejected|shadow|banned
  reason      TEXT,
  severity    INTEGER NOT NULL DEFAULT 0,
  created_by  TEXT,                -- 管理员标识（Header Token）
  created_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_moderation_user ON moderation_flags(user_hash, created_at DESC);
```

写入策略（UPSERT 与隐藏逻辑）：

```sql
-- 伪代码：在事务内完成
-- inputs: $uh=user_hash, $rks=total_rks, $kind=user_kind, $now, $sus=calculated_suspicion,
--         $shadow_threshold, $banned(bool), $details_json

-- 1) 最新提交入历史表 save_submissions（不依赖是否上榜）
INSERT INTO save_submissions(user_hash,total_rks,acc_stats,rks_jump,route,client_ip_hash,details_json,suspicion_score,created_at)
VALUES($uh,$rks,$acc_stats,$rks_jump,$route,$ip,$details_json,$sus,$now);

-- 2) 计算是否应隐藏（影子或封禁优先）
SET $hide = CASE WHEN $banned THEN 1 WHEN $sus >= $shadow_threshold THEN 1 ELSE 0 END;

-- 3) 首次入榜或提升最佳分才更新（保持 created_at 不变）
INSERT INTO leaderboard_rks(user_hash,total_rks,user_kind,suspicion_score,is_hidden,created_at,updated_at)
VALUES($uh,$rks,$kind,$sus,$hide,$now,$now)
ON CONFLICT(user_hash) DO UPDATE SET
  total_rks = CASE WHEN excluded.total_rks > leaderboard_rks.total_rks THEN excluded.total_rks ELSE leaderboard_rks.total_rks END,
  updated_at = CASE WHEN excluded.total_rks > leaderboard_rks.total_rks THEN excluded.updated_at ELSE leaderboard_rks.updated_at END,
  user_kind  = COALESCE(excluded.user_kind, leaderboard_rks.user_kind),
  suspicion_score = excluded.suspicion_score,
  is_hidden = CASE WHEN leaderboard_rks.is_hidden=1 OR excluded.is_hidden=1 THEN 1 ELSE 0 END;

-- 4) 可选：生成文字详情并缓存（依据展示权限）
INSERT INTO leaderboard_details(user_hash,rks_composition_json,best_top3_json,ap_top3_json,updated_at)
VALUES($uh,$rks_comp,$best3,$ap3,$now)
ON CONFLICT(user_hash) DO UPDATE SET
  rks_composition_json = COALESCE(excluded.rks_composition_json, leaderboard_details.rks_composition_json),
  best_top3_json       = COALESCE(excluded.best_top3_json,       leaderboard_details.best_top3_json),
  ap_top3_json         = COALESCE(excluded.ap_top3_json,         leaderboard_details.ap_top3_json),
  updated_at           = excluded.updated_at;
```

说明：
- 新成绩仅在“更优”时覆盖，防止倒退污染；
- 一旦被影子隐藏或封禁，`is_hidden` 持续为 1，直至管理端解除；
- `created_at` 表示首次上榜时间，便于审计；
- 文字详情可在写入路径生成后缓存，或于查询时按需计算（权衡性能）。

## 5. API 设计

说明：玩家端与公开端均为匿名可访问；玩家身份相关写接口通过 `UnifiedSaveRequest` 推导 `user_hash`。

### 5.1 玩家端

- GET `/leaderboard/rks/top?limit=100&offset=0`（支持 seek 分页）
  - 仅返回 `is_public=1 && is_hidden=0` 的玩家；
  - 排序稳定：`ORDER BY total_rks DESC, updated_at ASC, user_hash ASC`；
  - 大规模分页建议“seek 模式”：`?after_score=&after_user=`；
  - 若用户设置 `show_best_top3=true` 或 `show_ap_top3=true`，在返回项中附带对应文字数据；
  - 响应体示例：
    ```json
    {
      "items": [
        {
          "rank": 1,
          "alias": "Alice",
          "user": "ab12****",
          "score": 14.73,
          "updated_at": "2025-09-20T04:10:44Z",
          "best_top3": [
            { "song": "Tempestissimo", "difficulty": "AT", "acc": 99.43, "rks": 15.12 },
            { "song": "Another Song", "difficulty": "IN", "acc": 98.70, "rks": 14.88 },
            { "song": "Third", "difficulty": "HD", "acc": 98.20, "rks": 14.55 }
          ],
          "ap_top3": [
            { "song": "AP Song 1", "difficulty": "IN", "acc": 100.0, "rks": 13.45 },
            { "song": "AP Song 2", "difficulty": "AT", "acc": 100.0, "rks": 12.98 }
          ]
        }
      ],
      "total": 12345
    }
    ```

- GET `/leaderboard/rks/me`
  - 请求体：`UnifiedSaveRequest`（用于推导 `user_hash`）。
  - 排名定义：竞争排名（同分同名次）；`percentile = 100 * (1 - (rank-1)/total)`。
  - 响应体：
    ```json
    { "rank": 42, "score": 13.21, "total": 10000, "percentile": 99.58 }
    ```

- PUT `/leaderboard/alias`
  - 请求体：`{ "auth": UnifiedSaveRequest, "alias": "MyName" }`（幂等）。
  - 校验：长度 2~20；字符集 `[a-zA-Z0-9._-]`；大小写不敏感唯一；
  - 错误码：409（冲突）、422（非法别名）、401（认证无效）。

- PUT `/leaderboard/profile`
  - 请求体：`{ "auth": UnifiedSaveRequest, "is_public": true, "show_rks_composition": true, "show_best_top3": true, "show_ap_top3": true }`
  - 行为：公开/取消公开与文字展示权限；

### 5.2 公开端

- GET `/public/profile/{alias}`
  - 响应体（按用户展示权限返回字段）：
    ```json
    {
      "alias": "Alice",
      "score": 14.73,
      "updated_at": "...",
      "rks_composition": { "best27_sum": 390.12, "ap_top3_sum": 49.20 },
      "best_top3": [ { "song": "...", "difficulty": "...", "acc": 99.43, "rks": 15.12 } ],
      "ap_top3":   [ { "song": "...", "difficulty": "...", "acc": 100.0, "rks": 13.45 } ]
    }
    ```

- 可选 JSON 细分接口
  - GET `/public/profile/{alias}/rks-composition`
  - GET `/public/profile/{alias}/top3`

### 5.3 管理端（需 Header: `X-Admin-Token`）

- GET `/admin/leaderboard/suspicious?min_score=0.8&limit=100`
  - 返回 `suspicion_score >= min_score` 的最新提交/用户。

- POST `/admin/leaderboard/resolve`
  - 请求体：`{ "user_hash": "...", "status": "approved|rejected|shadow|banned", "reason": "..." }`

- POST `/admin/leaderboard/ban|unban`
  - 对 `leaderboard_rks.is_hidden` 与 `user_profile.is_public` 进行联动处理。

- POST `/admin/leaderboard/alias/force`
  - 强制设置/回收别名（仲裁冲突）。

## 6. 运行流程

1) 玩家调用 `/save` 成功解析
   - 计算 `total_rks` 与 `acc_stats`；
   - 取上次分数，计算 `rks_jump`；
   - 计算 `suspicion_score`（§7）；
   - 插入 `save_submissions`；
   - 若未封禁，则按“更优分数 + 隐藏逻辑”UPSERT `leaderboard_rks`；
   - 可在此时生成文字详情并写入 `leaderboard_details`（或延迟到查询时计算）。

2) 榜单查询
   - `top`：稳定排序与分页（必要时用 seek 模式）；
   - `me`：按定义计算 rank 与 percentile。

3) 公开资料
   - 别名唯一映射到 `user_hash`；仅公开 `is_public=1` 的用户；
   - 展示权限：`show_rks_composition`、`show_best_top3`、`show_ap_top3` 分别控制 JSON 返回内容。

## 7. 反作弊（可疑评分）

启发式评分，按权重累加，阈值触发影子隐藏或进入人工审核：

- 合法区间：
  - `acc_percent` 必须在 `[70.0, 100.0]`（异常：+0.2~0.5）。
  - `total_rks` 必须在合理上限内（依定数分布估计，超限：+0.5）。
- 速率/跳变：
  - `ΔRKS = |new_total - last_total|` 在短时间（如 10 分钟）> `0.5`：+0.3；> `1.0`：+0.8。
  - 高频更新（每分钟多次）：+0.2。
- 结构异常：
  - AP 比例异常高（>30% 且样本数少）：+0.3。
  - 很少曲目（<15）却超高 RKS：+0.4。
- 环境信号：
  - 短期多 `client_ip_hash` 切换：+0.2。

阈值建议：
- `suspicion_score >= 1.0` → `is_hidden=1`（影子隐藏，仍可在“我的名次”中看到自己的数值）。
- `0.6 <= score < 1.0` → 进入管理员审核队列。

凭证可信度加权（示例）：
- 官方 `sessionToken`：`suspicion_score -= 0.2`（最低不小于 0）。
- 外部凭证（platform/sessiontoken/apiUserId）：不加成。
- 加权仅影响自动影子隐藏，不影响管理员判定。

## 8. 别名与公开策略

- 别名规则：长度 2~20；字符集 `[a-zA-Z0-9._-]`；大小写不敏感（DDL 使用 `COLLATE NOCASE`）；保留字黑名单（如 `admin`, `system`, `null` 等）。
- 公开开关：`is_public`、`show_rks_composition`、`show_best_top3`、`show_ap_top3` 明确控制；撤销公开后：
  - 榜单与公开资料立即隐藏；
  - 文字详情可保留本地或清理（可配）。
- 展示名称优先级：`alias` > `hash 前缀`（如 `ab12****`）。
  - `hash 前缀` 推荐展示前 4 位 + `****`，避免可逆枚举。

## 9. 配置项（建议，不立即变更代码）

```toml
[leaderboard]
enabled = true
allow_public = true
# 成绩展示默认设置（纯文字）
default_show_rks_composition = true
default_show_best_top3 = true
default_show_ap_top3 = true

[leaderboard.anti_cheat]
enabled = true
suspicion_threshold = 1.0
review_threshold = 0.6

[leaderboard.admin]
tokens = ["changeme-force-override"]
```

> 最终实现可复用现有 `config.rs` 模式（环境变量覆盖：`APP_LEADERBOARD_*`）。

## 10. 监控与测试

- 监控：
  - 榜单写入 QPS、可疑评分分布、影子隐藏总量；
  - SQLite WAL 检查、数据库增长速率、碎片与备份计划。
- 测试：
  - 单测：upsert/rank、别名唯一、可疑评分边界、公开/隐藏切换、文字详情权限控制。
  - 集成：`/save → upsert → 设置公开/别名/展示权限 → /top → /public/profile/{alias}` 一致性（含 best_top3/ap_top3 返回）。
  - 压测：5k~50k 玩家分页性能（稳定排序 + seek 分页）。

## 11. 风险与缓解

- 刷分/作弊：先影子隐藏 + 审核；后续再演进规则与特征。
- 数据隐私：默认不公开；用户可自主选择展示哪些成绩信息，不存完整存档。
- 性能：
  - 排序分页走索引（`idx_lb_rks_order`）；
  - 文字详情可按需生成或缓存（`leaderboard_details`），避免在 `top` 请求中重复重算。
- 可靠性：写入失败时不影响查询，查询失败不影响上报；可配置重试与后台重建任务。

## 12. 开放问题

1) 是否并行提供 `ranking_score` 榜？
2) 是否需要周期榜（周/月）与分组榜（来源/地区/客户端）？
3) 公开撤销后，文字详情是否物理删除？
4) 管理员认证：先用静态多 token，是否限制来源网段？
