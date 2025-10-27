## TypeScript SDK 生成与使用指南（fetch 客户端）

本 SDK 基于 OpenAPI 从后端代码自动生成，覆盖全部公开接口（Auth/Save/Song/Image/Stats/Leaderboard/Health）。

目录结构：`sdk/ts`（可直接发布为 npm 包，或在本仓库内联使用）。

### 一键生成

1) 导出最新 OpenAPI 规范（无需启动服务）：

```
cargo run --example dump_openapi -q
# 生成的文件：sdk/openapi.json（UTF-8）
```

2) 生成并构建 TS SDK：

```
cd sdk/ts
pnpm i        # 或 npm/yarn
pnpm run generate
pnpm run build
```

生成产物在 `sdk/ts/dist`。

### 在项目中使用

```ts
import { OpenAPI, SongService, ImageService } from 'phi-backend-sdk';

// 必须设置 baseURL（可以是本机或你的部署地址）
OpenAPI.BASE = 'http://localhost:3939/api/v1';

// Song 示例（JSON）
const res = await SongService.searchSongs({ q: 'devil', unique: true });

// Image 示例（图片二进制，推荐 JPEG + 缩放以降低体积）
const resp = await fetch(`${OpenAPI.BASE}/image/bn?format=jpeg&width=800`, {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ sessionToken: '...', n: 30, theme: 'black', embed_images: false }),
});
const blob = await resp.blob(); // 在浏览器中即可显示或下载
```

说明：生成的服务类对应各 tag，例如 `SongService`、`LeaderboardService`、`StatsService` 等；图片接口是 `POST` 且返回二进制，SDK 仅提供类型辅助，发送与接收可用 `fetch` 或你已有的请求封装。

### 端点小抄

- Save：`POST /save`
- Auth：`GET /auth/qrcode`，`GET /auth/qrcode/{qr_id}/status`
- Song：`GET /songs/search`
- Image：`POST /image/bn`，`POST /image/song`，`POST /image/bn/user`
  - Query：`format=jpeg|png`，`width=<像素>`（按宽度同比例缩放）
- Leaderboard：`GET /leaderboard/top`，`GET /leaderboard/by-rank`，`POST /leaderboard/me`，`PUT /leaderboard/alias|profile`，`GET /leaderboard/public-profile`
- Stats：`GET /stats/summary`，`GET /stats/daily`
- Health：`GET /health`

### 再生成与升级

- 后端路由/结构变更后：

```
cargo run --example dump_openapi -q
cd sdk/ts && pnpm run generate && pnpm run build
```

- 如果你要发布到 npm：
  - 去除 `package.json` 中的 `private: true`
  - 增加 `name`、`version`、`repository` 等字段
  - 执行 `npm publish --access public`（或私有 registry）

### 常见问题

- 返回图片过大：使用 `format=jpeg&width=800` 显著减小；若仍大，可进一步减小 `width`。
- 跨域：部署时在后端开启 CORS（本仓库默认不强制设置）。
- 鉴权：如需管理接口鉴权，传入头 `X-Admin-Token: <token>`（SDK 暂未内置拦截器，可在调用处统一封装）。

