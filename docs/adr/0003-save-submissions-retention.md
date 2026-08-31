# ADR-0003：save_submissions 保留策略（D5——已接受，启用值已定 100）

- 日期：2026-09
- 状态：**已接受并启用**（机制落地 + owner 拍板 100，config.example 已设 `save_submissions_retention_per_user = 100`）
- 相关章节：docs/ARCHITECTURE.md §6（D5）

## 背景

`save_submissions`（存档提交流水）非时间序列、无归档，随玩家持续增长——是
"SQLite 无限膨胀"中最不受控的一张表（§6 D5）。但与 `events` 不同，它的行是
RKS 历史接口（`/rks/history`，`query_rks_history_page` 的 `(created_at,id)` 游标分页）
的数据源：**截断会缩短历史可回溯长度**，因此必须在"控增长"与"保历史"之间取舍。

## 决策

1. **机制（opt-in，默认关闭）**：新配置 `stats.save_submissions_retention_per_user`
   （默认 0 = 不清理，**零行为变化**）。>0 时每日聚合循环内调用
   `trim_save_submissions_per_user(keep, batch=5000)`：按
   `ROW_NUMBER() OVER (PARTITION BY user_hash ORDER BY created_at DESC, id DESC)`
   保留每用户最近 `keep` 条，其余分批删除（窗口函数库要求 SQLite >= 3.25 ✓）。
2. **归属**：单生产者调度器（每日聚合任务——Charter §4.6 调度纪律）；
   非 ad-hoc 线程。
3. **建议数值（待 owner 确认）**：`keep = 100`（DAU 100-200 下每用户年提交
   约 20-40 条，100 条 ≈ 3-5 年可回溯；存量增长被压到"每用户有界"）。
4. 影响评估：历史接口可回溯长度收窄到 `keep` 条/用户；`save_submissions` 表
   从"随总量无限增长"变为"O(活跃用户 × keep)"。`leaderboard_details` /
   `user_profile` / `moderation` 不动（各自小且需长期保留——不属本 ADR 范围）。

## 后果

- 正面：D5 单一最大膨胀源被限界；每用户数据量有界 → 分页/统计查询性能稳定。
- 负面：超过 `keep` 的旧提交不可回溯（前端如有"历史曲线"长跨度需求需 Product 确认）。
- 实施状态：配置字段、清理方法（impl-storage/submission.rs）、每日调度接入、
  单测（每用户保留最近 N 条）已落地；**启用 = 在 config.toml 设置一个非零值**。

## 待 owner 确认

- ~~`keep` 取值~~ —— **已决策：100（2026-09）**；如未来需要更长跨度曲线，走"归档提交流水到 Parquet"扩展（本 ADR §候选方案）。
- ~~是否需要"历史曲线长跨度"豁免~~ —— 当前评估不需要；如业务出现请以本 ADR 的 Parquet 扩展方案立项。
