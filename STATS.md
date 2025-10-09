# 统计功能使用说明（SQLite + Parquet）

本文档说明后端如何采集统计信息、如何配置与查询、以及如何在年末进行高维离线分析。

--------------------------------------------------------------------------------

## 能力总览

- 全量请求统计：对每个 HTTP 请求记录路由模板、方法、状态码、耗时等。
- 业务级打点：对关键业务事件上报 feature/action（如 BestN/单曲图生成、存档获取）。
- 去标识化用户维度：对用户标识和 IP 做 HMAC-SHA256 去敏（可配置盐值）。
- 在线明细存储：落地 SQLite（WAL）保存事件明细，零运维；提供简单聚合查询 API。
- 每日归档：按日导出明细为 Parquet（zstd 压缩），便于年末用 DuckDB/Polars 扫描做高维分析。

--------------------------------------------------------------------------------

## 数据如何被采集

1) 全局请求中间件
- 文件：`src/features/stats/middleware.rs`
- 触发：所有经过 Axum 的请求都会进入该中间件。
- 采集字段：
  - `ts_utc`（UTC 时间）
  - `route`（路由模板，如 `/image/bn`）
  - `method`、`status`、`duration_ms`
  - `client_ip_hash`（若配置了 `stats.user_hash_salt` 则从 `X-Forwarded-For`/`X-Real-IP` 去敏哈希）
  - `instance`（主机名）
- 行为：事件通过 `tokio::mpsc` 缓冲，按批插入 SQLite，队列打满时丢弃新明细以保护主流程。

2) 业务级打点
- 文件：
  - `src/features/image/handler.rs`（BestN/单曲图片生成成功后，分别上报 `feature=bestn|single_query` + `action=generate_image`）
  - `src/features/save/handler.rs`（成功获取并解析存档后，上报 `feature=save` + `action=get_save`）
- 用户去敏：基于请求中的 `UnifiedSaveRequest` 推导稳定标识，优先级：
  - `session_token` → `external_credentials.api_user_id` → `external_credentials.sessiontoken` → `platform:platform_id`
  - 使用 `HMAC_SHA256(stats.user_hash_salt, 标识)` 的前 16 字节（hex）作为 `user_hash`。
- 工具函数：`src/features/stats/mod.rs` 的 `derive_user_hash_from_auth()`。

--------------------------------------------------------------------------------

## 数据如何被存储与归档

1) SQLite 明细库（在线查询）
- 连接：`sqlx` + SQLite，默认启用 WAL，路径由 `stats.sqlite_path` 指定（默认 `./resources/usage_stats.db`）。
- 初始化与插入：`src/features/stats/storage.rs`
- 表结构：
  - `events(id, ts_utc, route, feature, action, method, status, duration_ms, user_hash, client_ip_hash, instance, extra_json)`
  - `daily_agg(date, feature, route, method, count, err_count)`（当前查询从 events 动态聚合，不强依赖该表）
- 索引：`ts_utc`、`(feature, ts_utc)`、`(route, ts_utc)`。

2) Parquet 每日归档（离线分析）
- 文件：`src/features/stats/archive.rs`
- 触发：每日本地时区 `stats.daily_aggregate_time`（默认 03:00）归档“前一日”的明细；也可手动触发。
- 产物：`resources/stats/v1/events/year=YYYY/month=MM/day=DD/events-<uuid>.parquet`
- 压缩：`stats.archive.compress`（默认 `zstd`）。

--------------------------------------------------------------------------------

## 如何配置

1) 配置文件 `config.toml`

```
[stats]
enabled = true
storage = "sqlite"
sqlite_path = "./resources/usage_stats.db"
sqlite_wal = true
batch_size = 100
flush_interval_ms = 1000
retention_hot_days = 180
daily_aggregate_time = "03:00"
user_hash_salt = ""          # 建议通过环境变量注入

[stats.archive]
parquet = true
dir = "./resources/stats/v1/events"
compress = "zstd"            # 可选：zstd|snappy|none
```

2) 环境变量覆盖（推荐）
- 例：PowerShell
  - `$env:APP_STATS_USER_HASH_SALT='your-secret-salt'`
  - `$env:APP_STATS_SQLITE_PATH='D:/data/phi/usage_stats.db'`
- 例：bash
  - `export APP_STATS_USER_HASH_SALT=your-secret-salt`
  - `export APP_STATS_SQLITE_PATH=/data/phi/usage_stats.db`

3) 关闭统计
- 将 `stats.enabled=false` 即可禁用所有采集与归档（路由仍可访问但返回空/错误）。

--------------------------------------------------------------------------------

## 在线查询与使用方式

1) 日聚合查询 API
- 路径：`GET {prefix}/stats/daily?start=YYYY-MM-DD&end=YYYY-MM-DD[&feature=bestn]`
- 返回：形如 `[{ date, feature, route, method, count, err_count }]`
- 用例（PowerShell）：
  - `iwr http://localhost:3939/api/v1/stats/daily?start=2025-08-20&end=2025-10-09&feature=bestn` | `ConvertFrom-Json`

2) 手动触发归档
- 路径：`POST {prefix}/stats/archive/now[?date=YYYY-MM-DD]`
- 默认归档昨天；可指定 `date`。

3) 快速汇总示例（从在线接口得到“BestN 2982、单曲 433”）
- Step 1：分别查询 bestn 与 single_query 的日聚合区间数据
- Step 2：对 `count` 求和即得区间总数

--------------------------------------------------------------------------------

## 年末离线分析（DuckDB / Polars）

1) DuckDB（SQL）

```sql
-- 启动 duckdb 后：
INSTALL parquet;
LOAD parquet;

-- 扫描全年分区
SELECT feature, action, COUNT(*) AS cnt
FROM read_parquet('resources/stats/v1/events/year=2025/**')
GROUP BY feature, action
ORDER BY cnt DESC;

-- 统计单用户 TopN 贡献（基于 user_hash，已去敏）
SELECT user_hash, feature, COUNT(*) cnt
FROM read_parquet('resources/stats/v1/events/year=2025/**')
WHERE user_hash IS NOT NULL
GROUP BY user_hash, feature
ORDER BY cnt DESC
LIMIT 50;
```

2) Python + Polars

```python
import polars as pl
df = pl.read_parquet('resources/stats/v1/events/year=2025/**')
print(
    df.group_by(['feature','action']).len().sort('len', descending=True)
)

# 分位数延迟
print(
    df.filter(pl.col('duration_ms').is_not_null())
      .group_by(['route','method'])
      .agg([
          pl.col('duration_ms').quantile(0.5).alias('p50'),
          pl.col('duration_ms').quantile(0.95).alias('p95'),
          pl.col('duration_ms').quantile(0.99).alias('p99'),
      ])
      .sort('p95', descending=True)
)
```

--------------------------------------------------------------------------------

## 运行与验证

- 构建运行：`cargo run`（建议设置 `RUST_LOG=phi_backend=debug` 观察日志）
- 首次运行：会初始化 SQLite 并按配置定时归档到 `resources/stats/v1/events`。
- 验证：
  - 访问 `/health`、`/docs` 等，随后在 `usage_stats.db` 的 `events` 查看新增记录。
  - 调用 `GET /stats/daily` 检查聚合结果是否合理。

--------------------------------------------------------------------------------

## 扩展与最佳实践

- 新增业务事件：
  - 引入 `StatsHandle`（在 `AppState` 中可取），调用：
    - `stats.track_feature("your_feature", "your_action", user_hash_opt, extra_json_opt).await;`
  - 推荐将业务维度写入 `extra_json`（字符串枚举、来源平台等非敏字段）。
- 去敏盐：务必通过环境变量注入 `APP_STATS_USER_HASH_SALT`，不要写入仓库。
- 仅路由模板：中间件记录的是模板（`MatchedPath`），避免把用户参数暴露到明细。
- 资源成本：Parquet 具备高压缩比；若年末分析需要跨机共享，直接复制整个 `resources/stats/v1/events` 目录即可。

--------------------------------------------------------------------------------

## 故障与性能

- 队列打满：丢弃新明细（不中断主流程）；日志会出现 `stats insert batch failed` 警告。
- SQLite 写入：已启用 WAL 和批量插入；在当前量级下足够稳定。
- 归档失败：每日任务失败会记录警告，不影响在线功能；可用手动接口重试。

--------------------------------------------------------------------------------

## 限制与注意事项

- 多实例：当前设计面向单实例部署；如未来横向扩展，建议切换到集中式数据库（PostgreSQL/TimescaleDB）或将明细经消息队列汇聚后统一写入。
- 隐私：未设置 `user_hash_salt` 时，不记录任何用户/IP 去敏标识；设置后也仅保存哈希，不保存原值。
- 安全：不要把盐或密钥写入配置文件；使用环境变量注入。

--------------------------------------------------------------------------------

## 代码参考路径（点击可直接打开）

- 中间件：`src/features/stats/middleware.rs`
- 业务打点工具与初始化：`src/features/stats/mod.rs`
- SQLite 存储：`src/features/stats/storage.rs`
- 每日归档（Parquet）：`src/features/stats/archive.rs`
- 统计查询 API：`src/features/stats/handler.rs`
- 业务打点示例：`src/features/image/handler.rs`、`src/features/save/handler.rs`
- 应用接入（main）：`src/main.rs`

