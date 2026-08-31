# ISSUE-0002：D5 save_submissions 保留策略——已解决

- 状态：**已解决**（2026-09：机制落地 + owner 拍板启用值 100；config.example 已设 `save_submissions_retention_per_user = 100`）
- 发现日期：2026-09
- 发现方式：架构评审（§6 D5：`save_submissions` 非时间序列、无归档，随玩家持续增长）
- 严重级：中
- 关联章节：ARCHITECTURE.md §6 D5；ADR-0003

## 问题陈述

`stats.save_submissions_retention_per_user` 机制已实现（默认 0 = 不清理，零行为变化）；
**启用数值（每用户保留最近 N 条）未定**——该值直接决定：表增长上限（O(活跃用户 × N)）
与 RKS 历史接口（/rks/history）可回溯长度。

## 证据

- 配置字段与 `trim_save_submissions_per_user`（窗口函数分批清理）+ 单测已合入；
- 影响评估已写入 ADR-0003（建议 N=100，约 3-5 年可回溯）。

## 影响评估

- 不启用：表持续膨胀（唯一无界增长源）。
- 启用（如 100）：表增长受限；超过 N 条的旧提交不可回溯。

## 候选方案

- 启用 N=100（推荐）；
- 若需长跨度历史曲线：提交流水归档到 Parquet（与 events 同管线，工作量 +1 轮）。

## 验收标准

1. config.toml 设置非零值后，每日维护日志出现"save_submissions 保留策略清理 N 条"；
2. /rks/history 分页与游标行为不受影响（仅可回溯长度收窄）。
