# 文档：排行榜（纯文字版）改动说明与后续项

作者：Codex（AI）  日期：2025-10-25

本文件简述本次引入的“纯文字版排行榜”改动、接口与配置，并罗列后续可选的特性（feat）。

**范围**
- 排行榜以总 RKS（Best27+AP3 平均）排序。
- 仅文本展示：RKS 构成（Best27+APTop3 汇总）与 BestTop3/APTop3 列表（曲名/难度/ACC/RKS）。
- 玩家公开资料（别名与展示开关）与基础反作弊（可疑分 + 影子隐藏）。
- 管理端接口（审核、别名仲裁）与 Swagger 安全方案。

**核心改动**
- 数据表与存储（自动 DDL，sqlite）
  - `leaderboard_rks`：用户最佳 RKS、隐藏位等。
  - `user_profile`：公开别名、展示开关（文字）。
  - `save_submissions`：上报历史与可疑分。
  - `leaderboard_details`：文字详情缓存（RKS 构成/BestTop3/APTop3）。
  - 路径：`src/features/stats/storage.rs`
- 写入链路（/save 解析成功后自动入榜）
  - 计算 `total_rks` 与文字详情；写 `save_submissions`；条件 UPSERT `leaderboard_rks`（仅更优覆盖，隐藏位保守为 1）；UPSERT `leaderboard_details`。
  - 位置：`src/features/save/handler.rs`
- 排行榜模块（接口与模型）
  - 模型：`src/features/leaderboard/models.rs`
  - 路由：`src/features/leaderboard/handler.rs`
  - 导出：`src/features/mod.rs`
- 接口路由与文档整合
  - 路由注册与 OpenAPI 文档：`src/main.rs`
  - 错误码扩展（422/409）：`src/error.rs`

**新增接口（玩家/公开）**
- GET `{api_prefix}/leaderboard/rks/top?limit&offset&after_score&after_updated&after_user`
  - 稳定排序：`score desc, updated_at asc, user_hash asc`；支持 seek 分页（after_*）。
  - 若开启展示，条目内附带 `best_top3` 与/或 `ap_top3` 文字数组。
- POST `{api_prefix}/leaderboard/rks/me`（body: `UnifiedSaveRequest`）
  - 返回 `{ rank, score, total, percentile }`（竞争排名）。
- PUT `{api_prefix}/leaderboard/alias`（幂等）
  - 别名校验失败返回 422，冲突返回 409。
- PUT `{api_prefix}/leaderboard/profile`
  - 更新 `is_public` 与三项文字展示开关；当 `leaderboard.allow_public=false` 时禁止公开。
- GET `/public/profile/{alias}`
  - 按展示开关返回 `rks_composition`、`best_top3`、`ap_top3`。

**新增接口（管理端）**
- GET `/admin/leaderboard/suspicious?min_score=0.6&limit=100`
- POST `/admin/leaderboard/resolve`（`approved|shadow|banned|rejected`）
- POST `/admin/leaderboard/alias/force`（从原持有者回收并赋予新别名）
- 统一鉴权：Header `X-Admin-Token`，令牌配置见下。

**配置项**
- `AppConfig.leaderboard`（`src/config.rs`）
  - `enabled`：是否启用排行榜（默认 true）
  - `allow_public`：是否允许公开资料（默认 true）
  - `default_show_rks_composition` / `default_show_best_top3` / `default_show_ap_top3`：别名首次创建时的三项默认展示开关（默认 true）
  - `admin_tokens`：管理员令牌列表（用于 `X-Admin-Token`）

**OpenAPI/Swagger**
- 顶层安全方案：ApiKey `AdminToken`（Header: `X-Admin-Token`），通过 `modifiers(&AdminTokenSecurity)` 注入组件。
- 管理端接口在文档中明确：需 `X-Admin-Token`，并设置 `security(("AdminToken" = []))`。

**测试**
- `tests/leaderboard_storage.rs`：验证 upsert 仅提升不回退。
- `src/features/leaderboard/handler.rs`（内部测试）：别名前缀遮蔽、管理员令牌校验。
- 现有性能测试签名已修正（仍 ignored）。

**兼容性**
- 自动创建新表，幂等初始化；不影响既有功能。
- 若 `stats.user_hash_salt` 未配置，仍可读排行榜，但无法稳定识别身份（入库会被跳过）。

**后续可选 feat**
- 排行榜：
  - 在 `/leaderboard/rks/top` 返回下一页游标（`next_after_*`），便于前端翻页。
  - 在路由层尊重 `leaderboard.enabled/allow_public`（当前仅在 profile 层限制公开）。
- 反作弊：
  - 将可疑打分权重/阈值参数化到配置；
  - 引入更多特征（时间窗 ΔRKS、AP 比例、样本量门槛、IP 变动等），并提供审计视图。
- 安全/治理：
  - 别名保留字/敏感词配置化；
  - 接口限流（特别是 alias/profile 与管理端）。
- 文档/可用性：
  - 在 Swagger UI 中添加示例请求体与示例 Header；
  - 为 `me` 增加错误语义（未上榜/未公开提示）。
- 测试：
  - 排名稳定性与 seek 分页边界测试；
  - 管理端接口集成测试与审计落库校验。

**快速使用**
- 配置 `leaderboard.admin_tokens = ["changeme-force-override"]`（config.toml）。
- 通过 `/save` 上报后：
  - 设置别名：`PUT {api_prefix}/leaderboard/alias`
  - 开启公开与展示开关：`PUT {api_prefix}/leaderboard/profile`
  - 查看榜单：`GET {api_prefix}/leaderboard/rks/top`
  - 查看公开资料：`GET /public/profile/{alias}`

