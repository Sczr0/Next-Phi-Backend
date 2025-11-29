# Phi Backend 未来演进路线图
> 日期：2025-11-29 · 撰写：Codex  
> 基于当前架构（Axum + utoipa + SQLite）与现有功能（扫码登录、存档/RKS、排行榜、图片渲染、统计）。

## 现状速写
- 路由齐备：auth/save/image/song/leaderboard/rks/stats/health 已在 OpenAPI 注册并挂载 Swagger UI。
- 鉴权模式：玩家基于 LeanCloud sessionToken 或 externalCredentials，管理员基于 `X-Admin-Token`。
- 数据面：统计与排行榜存 SQLite，事件与榜单写入路径集中在 save/rks/leaderboard。
- 可观察性：已有基础埋点（stats），但缺少统一日志/指标/告警规范。
- 文档：`API_USAGE.md` 已全量覆盖端点，但缺少架构/运营层面的演进规划。

## 演进目标
1) **稳定性**：保证高负载下的可用性与一致性。  
2) **可维护性**：降低认知成本，提升排障速度。  
3) **用户体验**：更快的接口响应与更友好的错误反馈。  
4) **安全合规**：完善鉴权、速率与数据保护。  
5) **可扩展性**：为后续多存储、多区域和新功能预留接口契约。

## 路线拆解

### 1. API 易用性与契约演进
- **短期（1-2 周）**
  - 在 `API_USAGE.md` 补充 externalCredentials 示例说明：最小字段组合与 TapTap / 其他平台的约定。
  - 为错误返回增加可选 JSON 包装开关（保持向后兼容），便于客户端解析。
- **中期（1-2 月）**
  - 引入 API 版本前缀策略（如 `/api/v2` 预留），支持灰度发布。
  - 自动生成 OpenAPI schema 变更 diff（pre-commit 脚本）并输出到 `docs/changelog.md`。
- **长期（季度）**
  - 设计“能力声明”端点（feature flags），让客户端自检可用功能与限流配额。

### 2. 安全与防滥用
- **短期**
  - 在 auth/save/image/leaderboard/rks 增加基础速率限制（按 IP + user_hash），默认值写入配置。
  - 管理端操作写审计日志表（admin_actions），记录 token 摘要与操作对象。
- **中期**
  - 对敏感写操作（alias/resolve）加入幂等键或请求签名，避免重复提交。
  - 支持可配置的水印/防盗链策略，针对公开图片与榜单文本输出。
- **长期**
  - 引入细粒度 RBAC（多管理员角色），并支持基于 JWT 的短期管理令牌轮换。

### 3. 性能与可用性
- **短期**
  - 为图片渲染与 save 解密增加内部 metrics（耗时分布、cache hit/miss）并暴露 `/metrics`（Prometheus）。
  - 优化排行榜查询：为 `leaderboard_rks(updated_at,total_rks,user_hash)` 添加复合索引检查。
- **中期**
  - 将 stats 与 leaderboard 持久化切换为可选 Postgres（配置驱动，迁移脚本放置 `migrations/`）。
  - 引入只读缓存层（如 Redis）缓存 Top/By-Rank 与公共档案，TTL + 失效策略。
- **长期**
  - 支持分区或分片的排行榜存储，预留跨区合并策略。

### 4. 可观察性与运维
- **短期**
  - 统一结构化日志格式（JSON line + trace_id），为关键路径增加 tracing span。
  - 在 `docs/DEPLOYMENT.md` 增补健康探针与系统d watchdog 配置最佳实践。
- **中期**
  - 增加慢查询日志与 SQLx 连接池监控指标；将 stats 归档作业结果写入事件表。
  - 建立“故障手册”模板，定位常见问题（数据库不可用、外部存档超时、图片缓存失效）。
- **长期**
  - 自动化容量评估（基于历史事件速率），生成扩容建议报表。

### 5. 开发效率与质量
- **短期**
  - 规范化 `cargo fmt`/`clippy --all-features -D warnings` 为提交前必跑脚本；新增 `make fmt lint test` 别名。
  - 在 `tests/` 补充接口级集成测试：auth 扫码状态流、排行榜 seek 分页、图片 Query 参数校验。
- **中期**
  - 引入 golden test（基准 JSON/文本快照）验证 API 兼容性，放置 `tests/golden/`。
  - 建立本地基准测试脚本：save 解密、BN 渲染、排行榜分页查询耗时。
- **长期**
  - 半自动回归：基于 OpenAPI 生成客户端并跑冒烟测试，输出到 `verification.md`。

### 6. 产品与体验
- **短期**
  - 公共档案新增“最近三次提交”简表，便于玩家自查。
  - 图片接口增加 `theme`/`nickname` 的可选回显字段，帮助客户端复用输入。
- **中期**
  - 排行榜开放“近期提升”视图（按 rks_jump 排序），可作为活动榜。
  - 提供“数据导出”接口（CSV/Parquet）给运营侧，需管理员鉴权。
- **长期**
  - 支持事件推送（WebSocket / SSE）用于扫码登录状态与存档处理进度提示。

## 依赖与前置检查
- 数据库：如引入 Postgres，需要迁移工具与配置切换策略；保留 SQLite 作为单机模式。
- 缓存：Redis 连接与失效策略需在 config 增补；确保统计/日志不会泄露敏感字段。
- 监控：若开启 Prometheus `/metrics`，需评估暴露口令与网段限制。

## 风险与缓解
- **向后兼容**：版本化 API 与 JSON 错误包装须默认关闭，提供灰度开关。
- **性能回退**：缓存与索引变更前应基准对比；提供开关回滚。
- **安全**：管理员接口新增写日志后注意敏感字段脱敏；水印/防盗链需兼顾合法嵌入场景。

## 里程碑建议
- M1（两周）：限流 + 监控初版 + 文档完善（externalCredentials/错误形态） + 索引检查。
- M2（两月）：Postgres 可选化、排行榜/档案缓存、灰度版本化、基准测试脚本。
- M3（季度）：RBAC、分区存储、事件推送、自动容量评估。

## 协作与跟踪
- 任务追踪：将以上事项拆分为 issue/任务卡，标注里程碑与 owner。
- 文档更新：落地变更时同步更新 `API_USAGE.md`、`DEPLOYMENT.md`、`verification.md` 与本路线图。
- 验证策略：所有变更需本地 `cargo fmt && cargo clippy --all-features -D warnings && cargo test` 通过；重要路径补充集成与基准测试。
