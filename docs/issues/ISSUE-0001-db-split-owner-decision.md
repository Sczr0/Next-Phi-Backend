# ISSUE-0001：D1 双库拆分（state.db / stats.db）——待 owner 决策实施时机

- 状态：待解决
- 发现日期：2026-09
- 发现方式：架构评审（总纲领 §6 D1：单库读写混池，后台聚合/归档与 API 热路径共用 `usage_stats.db` 一个池）
- 严重级：中
- 关联章节：ARCHITECTURE.md §6 D1；ADR-0002

## 问题陈述

`usage_stats.db` 同时承载：接口热路径（榜单/资料/封禁/会话黑名单）、高频埋点 append（`events`）、后台聚合/归档（DELETE+INSERT + VACUUM）。后台重写任务会与 API 读抢池（`busy_timeout=5s` 下请求可能被拖住数秒）。

## 证据

- `impl-storage` 的 `connect_sqlite` 单一池承载全部表（DDL 见 storage/connection.rs）；
- 每日聚合循环（root features/stats/mod.rs）与归档任务（archive.rs）与 API 并发执行。

## 影响评估

- 不拆：D1 症状持续（聚合大区间时 API 延迟毛刺）；D2-D7 已修复后这是最重的残余病灶。
- 拆：一个一次性迁移维护窗（预计 <5min）+ 双库备份；对外零变化（C1-C4 不受影响）。

## 候选方案

- **方案 A（ADR-0002 推荐）**：`usage_stats.db` 保留统计表（events/daily_*/归档），业务表迁入新 `state.db`（`stats.state_db_path` 配置）；迁移流程：拷贝 + **断言表集互斥** + VACUUM + integrity_check + 快照回滚。**推荐时机：随端口收口后拆（表归属已按域归位，只动池装配）**。

## 验收标准

1. 两库文件互斥（无同表共存）；
2. `cargo test` 全绿（金标准零改动）；
3. 维护窗演练一次（回滚路径 = 恢复快照 + 旧二进制）。
