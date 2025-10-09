# Phi Backend API 使用指南

## 快速开�?
### 启动服务�?
```bash
cargo run --release
```

服务器启动后，你会看到以下信息：
```
🚀 Phi-Backend 运行�?http://0.0.0.0:3939
📚 API 文档地址: http://0.0.0.0:3939/docs
🏥 健康检查地址: http://0.0.0.0:3939/health
💾 存档 API 地址: http://0.0.0.0:3939/api/v1/save
```

## API 端点

### 1. 健康检�?
**端点**: `GET /health`

**响应示例**:
```json
{
  "status": "healthy",
  "service": "phi-backend",
  "version": "0.1.0"
}
```

### 2. 获取存档数据

**端点**: `POST /api/v1/save`

**Content-Type**: `application/json`

#### 认证方式 1: 使用官方会话令牌

```bash
curl -X POST http://localhost:3939/api/v1/save \
  -H "Content-Type: application/json" \
  -d '{
    "sessionToken": "your-leancloud-session-token"
  }'
```

#### 认证方式 2: 使用外部 API 凭证（平台认证）

```bash
curl -X POST http://localhost:3939/api/v1/save \
  -H "Content-Type: application/json" \
  -d '{
    "externalCredentials": {
      "platform": "taptap",
      "platformId": "123456"
    }
  }'
```

### 3. 生成图片（Image APIs�?
- BN 图（Best N�?  - 端点: `POST {api_prefix}/image/bn`
  - Content-Type: `application/json`
  - 请求体示�?
    ```json
    {
      "n": 30,
      "theme": "black",
      "playerName": "Player A",
      "isUserGenerated": false,
      "records": [
        {
          "songId": "devillics",
          "songName": "Devillics",
          "difficulty": "IN",
          "score": 1000000,
          "acc": 99.53,
          "rks": 13.45,
          "difficultyValue": 13.7,
          "isFc": false
        }
      ]
    }
    ```
  - 响应: `image/png` 二进制字�?
- 单曲图（Song�?  - 端点: `POST {api_prefix}/image/song`
  - Content-Type: `application/json`
  - 请求体示�?
    ```json
    {
      "songId": "devillics",
      "songName": "Devillics",
      "playerName": "Player A",
      "difficultyScores": {
        "IN": { "score": 1000000, "acc": 99.53, "rks": 13.45, "difficultyValue": 13.7, "isFc": false, "isPhi": false }
      }
    }
    ```
  - 响应: `image/png` 二进制字�?
- RKS 排行榜图（Leaderboard�?  - 端点: `POST {api_prefix}/image/leaderboard`
  - Content-Type: `application/json`
  - 请求体示�?
    ```json
    {
      "title": "RKS 排行�?,
      "updateTime": "2024-01-01T00:00:00Z",
      "displayCount": 20,
      "entries": [
        { "playerName": "AAA", "rks": 12.34 }
      ]
    }
    ```
  - 响应: `image/png` 二进制字�?
#### 认证方式 3: 使用外部 API 凭证（会话令牌）

```bash
curl -X POST http://localhost:3939/api/v1/save \
  -H "Content-Type: application/json" \
  -d '{
    "externalCredentials": {
      "sessiontoken": "external-session-token"
    }
  }'
```

#### 认证方式 4: 使用外部 API 凭证（API 用户 ID�?
```bash
curl -X POST http://localhost:3939/api/v1/save \
  -H "Content-Type: application/json" \
  -d '{
    "externalCredentials": {
      "apiUserId": "user-id-123",
      "apiToken": "optional-token"
    }
  }'
```

**成功响应 (200 OK)**:
```json
{
  "data": {
    "gameRecord": { ... },
    "gameKey": { ... },
    "gameProgress": { ... },
    "user": { ... },
    "settings": { ... }
  }
}
```

**错误响应示例**:

- **400 Bad Request** - 参数错误
```json
{
  "error": "必须提供 sessionToken �?externalCredentials 中的一�?
}
```

- **400 Bad Request** - 凭证无效
```json
{
  "error": "外部凭证无效：必须提供以下凭证之一�?platform + platformId) �?sessiontoken �?apiUserId"
}
```

- **500 Internal Server Error** - 服务器错�?```json
{
  "error": "存档提供器错�? 网络错误: ..."
}
```

## 配置

### 配置文件 (config.toml)

```toml
[server]
host = "0.0.0.0"
port = 3939

[api]
prefix = "/api/v1"

[resources]
base_path = "./resources"
illustration_repo = "https://github.com/Catrong/phi-plugin-ill"
illustration_folder = "ill"

[logging]
level = "info"
format = "full"
```

### 环境变量覆盖

你可以使用环境变量覆盖配置文件中的值：

```bash
# 修改 API 前缀
export APP_API_PREFIX="/v2"

# 修改服务器端�?export APP_SERVER_PORT=8080

# 修改日志级别
export APP_LOGGING_LEVEL="debug"

cargo run
```

## API 文档

启动服务器后，访�?http://localhost:3939/docs 查看完整的交互式 API 文档（Swagger UI）�?
## 请求示例（使�?JavaScript�?
```javascript
// 使用官方会话令牌
const response = await fetch('http://localhost:3939/api/v1/save', {
  method: 'POST',
  headers: {
    'Content-Type': 'application/json',
  },
  body: JSON.stringify({
    sessionToken: 'your-session-token'
  })
});

const data = await response.json();
console.log(data);

// 使用外部 API 凭证
const response2 = await fetch('http://localhost:3939/api/v1/save', {
  method: 'POST',
  headers: {
    'Content-Type': 'application/json',
  },
  body: JSON.stringify({
    externalCredentials: {
      platform: 'taptap',
      platformId: '123456'
    }
  })
});

const data2 = await response2.json();
console.log(data2);
```

## 请求示例（使�?Python�?
```python
import requests
import json

# 使用官方会话令牌
response = requests.post(
    'http://localhost:3939/api/v1/save',
    headers={'Content-Type': 'application/json'},
    json={'sessionToken': 'your-session-token'}
)

print(response.json())

# 使用外部 API 凭证
response2 = requests.post(
    'http://localhost:3939/api/v1/save',
    headers={'Content-Type': 'application/json'},
    json={
        'externalCredentials': {
            'platform': 'taptap',
            'platformId': '123456'
        }
    }
)

print(response2.json())
```

## 注意事项

1. **认证方式互斥**: 不能同时提供 `sessionToken` �?`externalCredentials`
2. **外部凭证验证**: 使用外部凭证时，必须提供以下组合之一�?   - `platform` + `platformId`
   - `sessiontoken`
   - `apiUserId`（可选配�?`apiToken`�?3. **字段命名**: 请求体使�?camelCase 命名（如 `sessionToken`, `platformId`�?4. **超时设置**: 网络请求�?30-90 秒超时限�?5. **错误处理**: 建议实现重试机制以处理网络波�?
## 开发和调试

### 启用调试日志

```bash
export APP_LOGGING_LEVEL="debug"
cargo run
```

或修�?`config.toml`:
```toml
[logging]
level = "debug"
```

### 测试健康检�?
```bash
curl http://localhost:3939/health
```

### 查看 API 文档

浏览器访�? http://localhost:3939/docs




## 图片 API（更新）

- BN 图（Best N）
  - 端点: `POST {api_prefix}/image/bn`
  - Content-Type: `application/json`
  - 请求体: 与 `POST {api_prefix}/save` 的 `UnifiedSaveRequest` 相同（官方 sessionToken 或 externalCredentials）
  - Query: `?n=27`（可选，Best-N 数量，默认 27）
  - 响应: `image/png` 二进制字节

- 单曲图（Song）
  - 端点: `POST {api_prefix}/image/song`
  - Content-Type: `application/json`
  - 请求体: 与 `POST {api_prefix}/save` 的 `UnifiedSaveRequest` 相同
  - Query: `?song=<ID|名称|别名>`（唯一匹配，否则返回错误）
  - 响应: `image/png` 二进制字节

说明：Leaderboard 排行榜图片接口暂不提供。
