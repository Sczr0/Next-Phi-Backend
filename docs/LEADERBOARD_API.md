# 排行榜（纯文字版）API 速查

简要列出核心接口、参数与示例，便于快速联调。

- Base URL：`http://{host}:{port}`（默认 `3939`）
- API 前缀：`{prefix}`（默认 `/api/v1`）
- 路径示例：`http://localhost:3939/api/v1/leaderboard/rks/top`

## 公共与玩家接口

- GET `{prefix}/leaderboard/rks/top`
  - 说明：按总 RKS 排序的排行榜；稳定排序（score desc, updated_at asc, user_hash asc）
  - 并列说明：采用竞争排名（同分同名次），但接口按稳定排序切片，不扩展并列。若需完整并列，请拉取更大窗口后在客户端合并。
  - 支持游标：响应包含 `next_after_score` / `next_after_updated` / `next_after_user`，可用于下一页的 seek 查询。
  - Query：
    - `limit`（可选，默认 50，最大 200）
    - `offset`（可选，默认 0）
    - Seek 分页（可选）：`after_score`、`after_updated`、`after_user`
  - 响应：
    ```json
    {
      "items": [
        {
          "rank": 1,
          "alias": "Alice",
          "user": "ab12****",
          "score": 14.73,
          "updated_at": "2025-09-20T04:10:44Z",
          "best_top3": [{"song":"Tempestissimo","difficulty":"AT","acc":99.43,"rks":15.12}],
          "ap_top3":   [{"song":"AP Song","difficulty":"IN","acc":100.0,"rks":13.45}]
        }
      ],
      "total": 12345,
      "next_after_score": 14.73,
      "next_after_updated": "2025-09-20T04:10:44Z",
      "next_after_user": "abcd1234"
    }
    ```

- GET `{prefix}/leaderboard/rks/by-rank`（按排名区间）
  - 说明：根据 `rank` 或 `start+end` / `start+count`（1-based）获取玩家信息；返回结构同 TOP。
  - 并列说明：区间按位置切片，不扩展并列；如需包含所有并列项，请拉取更大区间后在客户端按稳定键合并。
  - 同样返回 `next_after_*` 游标，便于与 TOP 统一翻页策略。

- POST `{prefix}/leaderboard/rks/me`
  - 说明：根据认证信息推导用户，返回名次与百分位
  - Body（二选一示例）：
    ```json
    { "sessionToken": "r:abcdefg.hijklmn" }
    // 或
    { "externalCredentials": { "sessiontoken": "xxx" } }
    ```
  - 响应：`{ "rank": 42, "score": 13.21, "total": 10000, "percentile": 99.58 }`

- PUT `{prefix}/leaderboard/alias`
  - 说明：设置/修改公开别名（幂等）；大小写不敏感唯一
  - Body：
    ```json
    { "auth": { "sessionToken": "r:abcdefg.hijklmn" }, "alias": "Alice" }
    ```
  - 错误：409（别名占用）、422（别名非法）

- PUT `{prefix}/leaderboard/profile`
  - 说明：更新公开与展示开关；当配置 `leaderboard.allow_public=false` 时禁止公开
  - Body：
    ```json
    {
      "auth": { "sessionToken": "r:abcdefg.hijklmn" },
      "is_public": true,
      "show_rks_composition": true,
      "show_best_top3": true,
      "show_ap_top3": true
    }
    ```

- GET `{prefix}/public/profile/{alias}`
  - 说明：公开资料（尊重展示开关）
  - 响应示例：
    ```json
    {
      "alias": "Alice",
      "score": 14.73,
      "updated_at": "2025-09-20T04:10:44Z",
      "rks_composition": { "best27_sum": 390.12, "ap_top3_sum": 49.20 },
      "best_top3": [{"song":"Tempestissimo","difficulty":"AT","acc":99.43,"rks":15.12}],
      "ap_top3": [{"song":"AP Song","difficulty":"IN","acc":100.0,"rks":13.45}]
    }
    ```

## 管理端接口

- 鉴权：在请求头添加 `X-Admin-Token: {token}`；令牌来源 `config.leaderboard.admin_tokens`。

- GET `{prefix}/admin/leaderboard/suspicious?min_score=0.6&limit=100`
  - 说明：按可疑分降序列出用户
  - Header：`X-Admin-Token: changeme-force-override`
  - 响应：
    ```json
    [{
      "user": "ab12****",
      "alias": "Alice",
      "score": 14.73,
      "suspicion": 1.1,
      "updated_at": "2025-09-20T04:10:44Z"
    }]
    ```

- POST `{prefix}/admin/leaderboard/resolve`
  - 说明：审核可疑用户状态
  - Header：`X-Admin-Token: {token}`
  - Body：`{ "user_hash":"abcde12345", "status":"shadow", "reason":"suspicious jump" }`
  - 备注：`status` 取值 `approved|shadow|banned|rejected`

- POST `{prefix}/admin/leaderboard/alias/force`
  - 说明：强制设置/回收别名（从原持有人移除）
  - Header：`X-Admin-Token: {token}`
  - Body：`{ "user_hash":"abcde12345", "alias":"Alice" }`

## cURL 示例

- 获取 TOP（前 10）
```bash
curl "http://localhost:3939/api/v1/leaderboard/rks/top?limit=10"
```

- 我的名次（使用 sessionToken）
```bash
curl -X POST "http://localhost:3939/api/v1/leaderboard/rks/me" \
  -H "Content-Type: application/json" \
  -d '{"sessionToken":"r:abcdefg.hijklmn"}'
```

- 设置别名
```bash
curl -X PUT "http://localhost:3939/api/v1/leaderboard/alias" \
  -H "Content-Type: application/json" \
  -d '{"auth":{"sessionToken":"r:abcdefg.hijklmn"},"alias":"Alice"}'
```

- 开启公开与展示开关
```bash
curl -X PUT "http://localhost:3939/api/v1/leaderboard/profile" \
  -H "Content-Type: application/json" \
  -d '{"auth":{"sessionToken":"r:abcdefg.hijklmn"},"is_public":true,"show_rks_composition":true,"show_best_top3":true,"show_ap_top3":true}'
```

- 公开资料
```bash
curl "http://localhost:3939/api/v1/public/profile/Alice"
```

- 管理：可疑列表
```bash
curl "http://localhost:3939/api/v1/admin/leaderboard/suspicious?min_score=0.8&limit=20" \
  -H "X-Admin-Token: changeme-force-override"
```

- 管理：审核
```bash
curl -X POST "http://localhost:3939/api/v1/admin/leaderboard/resolve" \
  -H "Content-Type: application/json" \
  -H "X-Admin-Token: changeme-force-override" \
  -d '{"user_hash":"abcde12345","status":"shadow","reason":"jump too large"}'
```

## 配置速览（config.toml）
```toml
[leaderboard]
enabled = true
allow_public = true
admin_tokens = ["changeme-force-override"]
# 默认展示开关
default_show_rks_composition = true
default_show_best_top3 = true
default_show_ap_top3 = true
```
