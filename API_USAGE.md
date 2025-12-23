# Phi Backend API 使用指南
> 更新日期：2025-11-29 · 编写：Codex

## 快速开始
- 启动：`cargo run`（默认监听 `0.0.0.0:3939`）
- API 前缀：`/api/v1`（可用环境变量 `APP_API_PREFIX` 覆盖）
- Swagger UI：`http://localhost:3939/docs`
- OpenAPI JSON：`http://localhost:3939/api-docs/openapi.json`
- 健康检查：`GET http://localhost:3939/health`（无前缀）

## 通用约定与鉴权
- 请求：UTF-8 编码，`POST/PUT` 请带 `Content-Type: application/json`。
- 错误：非 2xx 返回**纯文本**错误信息；典型状态码：202(待授权)、400/401、404、409、422、500、502。
- 玩家鉴权（任选其一；至少提供 `sessionToken`，或 `platform+platformId`，或 `sessiontoken`，或 `apiUserId`）：
  ```json
  { "sessionToken": "r:abcdefg.hijklmn" }
  {
    "externalCredentials": {
      "platform": "TapTap",
      "platformId": "user_123",
      "sessiontoken": "ext-session",
      "apiUserId": "1008611",
      "apiToken": "token-xyz"
    },
    "taptapVersion": "cn" // 可选：cn|global，默认 cn
  }
  ```
- 管理员鉴权：在请求头添加 `X-Admin-Token: <token>`（来源 `config.leaderboard.admin_tokens`）。
- 图片输出统一 Query 选项：`format=png|jpeg|webp`（默认 png）、`width=<px>`（默认 1200）、`webp_quality=1-100`（默认 80，webp 时有效）、`webp_lossless=true|false`（默认 false）。

## 端点清单

### 健康检查
- `GET /health` → `{ "status": "healthy", "service": "phi-backend", "version": "…" }`

### TapTap 扫码登录
- `GET {prefix}/auth/qrcode?[version=cn|global]`  
  返回 `qr_id`、`verification_url`、`qrcode_base64`(SVG data URL)。
- `GET {prefix}/auth/qrcode/{qr_id}/status`  
  返回 `status`(Pending/Confirmed/Error/Expired)，可含 `session_token` 与 `retry_after`；二维码失效返回 404。
- `POST {prefix}/auth/user-id`  
  Body：`UnifiedSaveRequest` → `{ "user_id": "32-hex", "user_kind": "session_token|external_api_user_id|external_sessiontoken|platform_pair" }`。  
  注意：该端点依赖 `stats.user_hash_salt`，请通过环境变量 `APP_STATS_USER_HASH_SALT` 配置，否则无法生成稳定的 user_id。

### 存档 / RKS
- `POST {prefix}/save?[calculate_rks=true|false]`  
  Body：`UnifiedSaveRequest`。`calculate_rks=true` 时返回 `{ "save": {...}, "rks": {...} }`，否则 `{ "data": {...} }`。
- `POST {prefix}/rks/history`  
  Body：`{ "auth": UnifiedSaveRequest, "limit": 50, "offset": 0 }`（limit 默认 50，最大 200）。返回 `items[{rks,rks_jump,created_at}], total, current_rks, peak_rks`。

### 图片
> 响应类型 `image/png` / `image/jpeg` / `image/webp`（由 Query 控制）。
- `POST {prefix}/image/bn`  
  Body：`{ "auth": {...}, "n": 30, "theme": "black|white", "embed_images": false, "nickname": "可选" }`
- `POST {prefix}/image/song`  
  Body：`{ "auth": {...}, "song": "曲目ID或名称", "embed_images": false, "nickname": "可选" }`
- `POST {prefix}/image/bn/user`（无需存档）  
  Body：`{ "theme": "black|white", "nickname": "可选", "unlock_password": "可选", "scores": [{ "song": "...", "difficulty": "EZ|HD|IN|AT", "acc": 99.5, "score": 1000000 }] }`

### 歌曲搜索
- `GET {prefix}/songs/search?q=keyword&unique=true|false`  
  `unique=true`：未命中返回 404，多命中返回 409；否则返回列表或单个结果。

### 排行榜（玩家）
- `GET {prefix}/leaderboard/rks/top?[limit=50&offset=0]`（或使用 `after_score&after_updated&after_user` 进行 seek 分页，limit 1~200） → `items[{rank,alias?,user,score,updated_at,best_top3?,ap_top3?}], total, next_after_*`
- `GET {prefix}/leaderboard/rks/by-rank?rank=10` 或 `start=1&end=20` / `start=1&count=20`（count ≤ 200）→ 同上结构
- `POST {prefix}/leaderboard/rks/me`  
  Body：`UnifiedSaveRequest` → `{rank, score, total, percentile}`
- `PUT {prefix}/leaderboard/alias`  
  Body：`{ "auth": {...}, "alias": "自定义名" }`；长度 2~20，仅限 字母/数字/中日韩/._-，保留字：`admin/system/null/undefined/root`
- `PUT {prefix}/leaderboard/profile`  
  Body：`{ "auth": {...}, "is_public": true|false, "show_rks_composition": true|false, "show_best_top3": true|false, "show_ap_top3": true|false }`
- `GET {prefix}/public/profile/{alias}` → 公共档案：`alias, score, updated_at, rks_composition?, best_top3?, ap_top3?`

### 管理端（需 `X-Admin-Token`）
- `GET {prefix}/admin/leaderboard/suspicious?[min_score=0.6&limit=100]`（limit 1~500）→ `[{user, alias?, score, suspicion, updated_at}]`
- `POST {prefix}/admin/leaderboard/resolve`  
  Body：`{ "user_hash": "...", "status": "approved|shadow|banned|rejected", "reason": "可选" }`；shadow/banned/rejected 会隐藏榜单条目。
- `POST {prefix}/admin/leaderboard/alias/force`  
  Body：`{ "user_hash": "...", "alias": "..." }`；与普通别名相同校验，提交后会回收原持有者的同名别名。

### 统计
- `GET {prefix}/stats/summary` → `timezone, config_start_at, first_event_at, last_event_at, features[{feature,count,last_at}], unique_users{total,by_kind}`
- `GET {prefix}/stats/daily?start=YYYY-MM-DD&end=YYYY-MM-DD[&feature=bestn]` → `[{date, feature, route, method, count, err_count}]`
- `POST {prefix}/stats/archive/now?[date=YYYY-MM-DD]`（默认昨天）→ `{"ok": true, "date": "YYYY-MM-DD"}`

## 示例请求
```bash
# 1) 获取存档并计算 RKS
curl -X POST "http://localhost:3939/api/v1/save?calculate_rks=true" \
  -H "Content-Type: application/json" \
  -d '{ "sessionToken": "r:your-leancloud-session-token" }'

# 2) 生成 BestN WebP 图并保存到本地
curl -X POST "http://localhost:3939/api/v1/image/bn?format=webp&width=1200" \
  -H "Content-Type: application/json" \
  -o bestn.webp \
  -d '{ "auth": { "sessionToken": "r:your-leancloud-session-token" }, "n": 30, "theme": "black" }'

# 3) 搜索歌曲（要求唯一命中）
curl "http://localhost:3939/api/v1/songs/search?q=devil&unique=true"

# 4) 设置排行榜别名
curl -X PUT "http://localhost:3939/api/v1/leaderboard/alias" \
  -H "Content-Type: application/json" \
  -d '{ "auth": { "sessionToken": "r:your-leancloud-session-token" }, "alias": "Alice" }'

# 5) 管理员标记可疑用户
curl -X POST "http://localhost:3939/api/v1/admin/leaderboard/resolve" \
  -H "Content-Type: application/json" \
  -H "X-Admin-Token: <token>" \
  -d '{ "user_hash": "abcd1234", "status": "shadow", "reason": "suspicious jump" }'
```
