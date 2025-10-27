# Phi Backend TypeScript SDK\n\n- 自动生成于 OpenAPI（路径：../openapi.json）\n- 客户端实现：原生 fetch\n\n## 安装\n\n在 sdk/ts 目录：\n\n`ash\npnpm i # 或 npm i / yarn\npnpm run generate\npnpm run build\n`\n\n## 使用\n\n`	s
import { OpenAPI, ImageService, SaveService, SongService, LeaderboardService, StatsService } from 'phi-backend-sdk';

// 配置服务地址（必须）
OpenAPI.BASE = 'http://localhost:3939/api/v1';

// BN 图片（JPEG + 指定宽度）
const resp = await fetch(${OpenAPI.BASE}/image/bn?format=jpeg&width=800, {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ sessionToken: '...', n: 30, theme: 'black', embed_images: false }),
});
const blob = await resp.blob();

// 其余 JSON 接口可直接用服务类
const songs = await SongService.searchSongs({ q: 'devil', unique: true });
`
\n## 端点一览\n- Save：POST /save\n- Auth：POST /auth/qrcode, GET /auth/qrcode/status\n- Song：GET /songs/search\n- Image：POST /image/bn, /image/song, /image/bn/user（支持 query: format, width）\n- Leaderboard：GET /leaderboard/top, GET /leaderboard/by-rank, POST /leaderboard/me, PUT /leaderboard/alias, PUT /leaderboard/profile, GET /leaderboard/public-profile\n- Stats：GET /stats/summary, GET /stats/daily\n\n## 再生成\n- 修改后端接口后：\n`ash\n# 在仓库根目录：
cargo run --example dump_openapi -q > sdk/openapi.json
# 在 sdk/ts 目录：
pnpm run generate && pnpm run build
`\n
