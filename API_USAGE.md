# Phi Backend API 使用指南

## 快速开始
- 启动服务：`cargo run`
- Swagger UI：`http://localhost:3939/docs`
- 健康检查：`GET /health`
- 默认 API 前缀：`/api/v1`（可通过 `APP_API_PREFIX` 覆盖）

## API 端点

### 健康检查
- 端点：`GET /health`
- 用途：探活与版本信息

### 存档 API（Save）
- 端点：`POST {api_prefix}/save`
- 认证：二选一
  - 官方会话：请求体 `{ "sessionToken": "..." }`
  - 外部凭证：请求体 `{ "externalCredentials": { ... } }`
- 可选参数：`calculate_rks=true|false`（查询字符串）
- 成功：返回解析后的存档，或附带玩家 RKS 概览

示例：
```bash
curl -X POST "http://localhost:3939/api/v1/save?calculate_rks=true" \
  -H "Content-Type: application/json" \
  -d '{ "sessionToken": "your-leancloud-session-token" }'
```

### 图片 API（Image）
- BN 图：`POST {api_prefix}/image/bn`（从存档生成 BestN 汇总图）
- 单曲图：`POST {api_prefix}/image/song`（从存档生成单曲图片）
- 用户自报 BN 图：`POST {api_prefix}/image/bn/user`（无需存档，直接提交成绩生成 BestN）
- 响应均为 `image/png`

### 歌曲检索（Song）
- 端点：`GET {api_prefix}/songs/search`
- 参数：
  - `q` 必填：歌曲 ID/名称/别名，支持模糊匹配
  - `unique` 可选：`true|false`，`true` 时要求唯一匹配（未命中 404，多命中 409）
```bash
curl "http://localhost:3939/api/v1/songs/search?q=devil&unique=true"
```

### 统计 API（Stats）

1) 汇总
- 端点：`GET {api_prefix}/stats/summary`
- 返回：首末事件时间、各功能的使用次数与最近时间、唯一用户统计（总量与来源分布）
- 功能次数统计中的“功能名”可能值：
  - `bestn`：生成 BestN 汇总图
  - `bestn_user`：生成用户自报 BestN 图片
  - `single_query`：生成单曲成绩图
  - `save`：获取并解析玩家存档
  - `song_search`：歌曲检索
- 备注：功能统计只包含通过业务打点上报的功能；路由级请求数请使用日聚合接口。

2) 日聚合
- 端点：`GET {api_prefix}/stats/daily?start=YYYY-MM-DD&end=YYYY-MM-DD[&feature=bestn]`
- 返回：`[{ date, feature, route, method, count, err_count }]`

## 配置与环境
- 配置文件：`config.toml`
- 常用覆盖：
  - `APP_API_PREFIX` 调整 API 前缀
  - `APP_STATS_USER_HASH_SALT` 启用去敏化用户哈希
  - `APP_LOGGING_LEVEL` 调整日志级别

## 说明
- 仓库所有字符串与文件均为 UTF-8 编码
- 更多统计细节参考：`STATS.md`

