# Phi Backend TypeScript SDK\n\n- �Զ������� OpenAPI��·����../openapi.json��\n- �ͻ���ʵ�֣�ԭ�� fetch\n\n## ��װ\n\n�� sdk/ts Ŀ¼��\n\n`ash\npnpm i # �� npm i / yarn\npnpm run generate\npnpm run build\n`\n\n## ʹ��\n\n`	s
import { OpenAPI, ImageService, SaveService, SongService, LeaderboardService, StatsService } from 'phi-backend-sdk';

// ���÷����ַ�����룩
OpenAPI.BASE = 'http://localhost:3939/api/v1';

// BN ͼƬ��JPEG + ָ����ȣ�
const resp = await fetch(${OpenAPI.BASE}/image/bn?format=jpeg&width=800, {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ sessionToken: '...', n: 30, theme: 'black', embed_images: false }),
});
const blob = await resp.blob();

// ���� JSON �ӿڿ�ֱ���÷�����
const songs = await SongService.searchSongs({ q: 'devil', unique: true });
`
\n## �˵�һ��\n- Save��POST /save\n- Auth��POST /auth/qrcode, GET /auth/qrcode/status\n- Song��GET /songs/search\n- Image��POST /image/bn, /image/song, /image/bn/user��֧�� query: format, width��\n- Leaderboard��GET /leaderboard/top, GET /leaderboard/by-rank, POST /leaderboard/me, PUT /leaderboard/alias, PUT /leaderboard/profile, GET /leaderboard/public-profile\n- Stats��GET /stats/summary, GET /stats/daily\n\n## ������\n- �޸ĺ�˽ӿں�\n`ash\n# �ڲֿ��Ŀ¼��
cargo run --example dump_openapi -q > sdk/openapi.json
# �� sdk/ts Ŀ¼��
pnpm run generate && pnpm run build
`\n
