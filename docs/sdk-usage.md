# Phi Backend TypeScript SDK 使用手册（详细版）

本文介绍如何在你的插件或前端/后端项目中使用本仓库生成的 TypeScript SDK（基于 OpenAPI 自动生成，fetch 客户端）。

位置与结构
- OpenAPI 规范：`sdk/openapi.json`
- SDK 源码：`sdk/ts/src`
- SDK 构建产物：`sdk/ts/dist`
- 包配置：`sdk/ts/package.json`

兼容环境
- Node.js 18+（内置 fetch）/ 20+/22+（推荐）
- 浏览器（需后端允许 CORS）
- 打包器：Vite、Webpack、esbuild 均可

一、生成与构建
- 导出最新 OpenAPI（无需启动服务）
  - `cargo run --example dump_openapi -q`
  - 输出：`sdk/openapi.json`
- 生成 + 构建 SDK（在 `sdk/ts` 目录）
  - `pnpm i`
  - `pnpm run generate`
  - `pnpm run build`

二、在项目里引用
- 直接引用构建产物（建议）
  - `import { OpenAPI, SongService, ImageService } from '<repo>/sdk/ts/dist/index.js'`
  - TypeScript 会自动加载 `dist/index.d.ts`
- 作为包使用（monorepo/workspace）
  - 将 `sdk/ts` 加入你的工作区（pnpm/yarn workspace），或用 `pnpm link`

三、基础配置
- 基地址（必填）
  - `OpenAPI.BASE = 'http://localhost:3939/api/v1'`
- 鉴权头（可选，管理接口）
  - `OpenAPI.HEADERS = { 'X-Admin-Token': '<token>' }`
- 凭据/跨域（浏览器）
  - `OpenAPI.WITH_CREDENTIALS = true`（如需携带 cookie）
  - CORS 需要后端放行

四、调用示例
1) 歌曲检索（JSON）
```ts
import { OpenAPI, SongService } from '<repo>/sdk/ts/dist/index.js';
OpenAPI.BASE = 'http://localhost:3939/api/v1';

const res = await SongService.searchSongs({ q: 'devil', unique: true });
console.log(res.items); // 结果数组
```

2) 存档解析（JSON）
```ts
import { SaveService } from '<repo>/sdk/ts/dist/index.js';
const save = await SaveService.getSaveData({ requestBody: { sessionToken: 'your-leancloud-session-token' } });
console.log(save);
```

3) 排行榜（JSON）
```ts
import { LeaderboardService } from '<repo>/sdk/ts/dist/index.js';
const top = await LeaderboardService.getTop({ count: 50 });
console.log(top.items);
```

4) 统计（JSON）
```ts
import { StatsService } from '<repo>/sdk/ts/dist/index.js';
const summary = await StatsService.getStatsSummary();
console.log(summary);
```

5) 图片接口（二进制）
- 图片接口返回 PNG/JPEG 二进制，SDK 仅提供类型辅助；建议直接用 fetch：
```ts
import { OpenAPI } from '<repo>/sdk/ts/dist/index.js';
OpenAPI.BASE = 'http://localhost:3939/api/v1';

// BN 图片（低带宽建议 JPEG + 缩放）
const resp = await fetch(`${OpenAPI.BASE}/image/bn?format=jpeg&width=800`, {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ sessionToken: '...', n: 30, theme: 'black', embed_images: false }),
});
if (!resp.ok) {
  // 尝试解析后端错误
  const err = await resp.json().catch(() => ({}));
  throw new Error(`Image error ${resp.status}: ${JSON.stringify(err)}`);
}
// 浏览器：使用 blob
const blob = await resp.blob();
// Node：保存为文件
const arrayBuffer = await resp.arrayBuffer();
await import('node:fs/promises').then(fs => fs.writeFile('bn.jpg', Buffer.from(arrayBuffer)));
```

6) 单曲图片 / 用户自报 BN（同理）
```ts
// 单曲图
await fetch(`${OpenAPI.BASE}/image/song?format=jpeg&width=800`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ sessionToken: '...', song: 'Arcahv', embed_images: false }) });

// 用户自报 BN 图
await fetch(`${OpenAPI.BASE}/image/bn/user?format=jpeg&width=800`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ theme: 'black', scores: [{ song: 'devil', difficulty: 'IN', acc: 98.5, score: 995000 }] }) });
```

五、错误处理与超时
- JSON 接口
  - 服务类方法返回 `Promise<T>`；失败会抛出 `ApiError`（包含 `status`、`body`）
  - 示例：
```ts
import { ApiError, SongService } from '<repo>/sdk/ts/dist/index.js';
try {
  const data = await SongService.searchSongs({ q: 'devil' });
} catch (e) {
  if (e instanceof ApiError) {
    console.error(e.status, e.body);
  }
}
```
- 超时取消（fetch）
  - 使用 AbortController：
```ts
const ctrl = new AbortController();
const timer = setTimeout(() => ctrl.abort(), 10_000);
try {
  const resp = await fetch(`${OpenAPI.BASE}/image/bn?format=jpeg&width=800`, { method: 'POST', body: '...', signal: ctrl.signal });
  // ...
} finally {
  clearTimeout(timer);
}
```
- JSON 服务也支持 options（本 SDK 生成时启用了 `--useOptions`），可按需要传入 `signal`：
```ts
const ctrl = new AbortController();
await SongService.searchSongs({ q: 'devil' }, { signal: ctrl.signal });
```

六、图片体积优化（强烈建议）
- Query：`format=jpeg|png`，`width=<像素>`（等比缩放）
- 建议：`format=jpeg&width=800`（大幅降低返回字节数）
- 服务器端已针对不同 `format/width` 做缓存分片

七、在聊天插件中的实践建议
- 输出到聊天平台通常需要 Buffer 或 URL：
  - Buffer：使用 `resp.arrayBuffer()` → `Buffer.from()`
  - URL：将图片上传到对象存储或平台提供的上传接口
- 避免走你带宽较低的 ECS 上行，优先选择 JPEG + 缩放；或将图片上传到平台的媒体存储再引用

八、再生成与升级
- 后端改动后：
  - `cargo run --example dump_openapi -q`
  - `cd sdk/ts && pnpm run generate && pnpm run build`
- 如需发布到 npm：参考 `sdk/ts/README.md` 或本仓库说明（不发布时可忽略）

九、常见问题
- 403/401：检查 `X-Admin-Token` 或其它鉴权
- CORS：浏览器跨域需要后端允许；开发时可通过反向代理解决
- 图片错误解析：`resp.ok` 为 false 时尝试 `await resp.json()` 得到 `AppError`

