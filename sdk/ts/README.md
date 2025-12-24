# Phi Backend TypeScript SDK

> 更新日期：2025-12-24 · 编写：Codex

本目录为根据后端 OpenAPI 自动生成的 TypeScript SDK（fetch 客户端），用于前端/插件快速调用 Phi Backend 的 JSON API。

- OpenAPI 规范：`sdk/openapi.json`
- 代码生成：`sdk/ts/src`（`openapi-typescript-codegen`）
- 构建产物：`sdk/ts/dist`

## 生成

在仓库根目录导出 OpenAPI（无需启动服务）：

```bash
cargo run --example dump_openapi -q
```

在 `sdk/ts` 目录生成并构建：

```bash
pnpm i        # 或 npm i / yarn
pnpm run generate
pnpm run build
```

## 使用

```ts
import { OpenAPI, SongService } from 'phi-backend-sdk';

// 业务接口默认挂载在 /api/v1（见 config.api.prefix）
OpenAPI.BASE = 'http://localhost:3939/api/v1';

const res = await SongService.searchSongs({ q: 'devil', unique: false });
console.log(res);
```

图片接口返回二进制（png/jpeg/webp）或 SVG 文本（`format=svg`）。更推荐直接用 `fetch`：

```ts
const resp = await fetch(`${OpenAPI.BASE}/image/bn?format=jpeg&width=800`, {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({
    sessionToken: '...',
    n: 30,
    theme: 'black',
    embed_images: false,
  }),
});
if (!resp.ok) throw new Error(await resp.text()); // 非 2xx 为 text/plain
const blob = await resp.blob();
```

## 端点速查（相对 OpenAPI.BASE）

- Save：`POST /save`
- Auth：`GET /auth/qrcode`，`GET /auth/qrcode/{qr_id}/status`，`POST /auth/user-id`
- Song：`GET /songs/search`
- Image：`POST /image/bn`，`POST /image/song`，`POST /image/bn/user`
- Leaderboard：`GET /leaderboard/rks/top`，`GET /leaderboard/rks/by-rank`，`POST /leaderboard/rks/me`，`PUT /leaderboard/alias`，`PUT /leaderboard/profile`，`GET /public/profile/{alias}`
- Stats：`GET /stats/summary`，`GET /stats/daily`，`POST /stats/archive/now`

管理端接口需要请求头 `X-Admin-Token`（详见 `docs/LEADERBOARD_API.md`）。

