# 图片输出格式与统一编码入口（2025-11-01, Codex）

本项目的图片接口（BN、单曲、用户自报 BN）支持通过统一的查询参数控制输出格式与编码参数；同时，服务端内部提供统一的编码入口函数，便于在代码中按需调用。

## 客户端调用（查询参数）

- `format`：`png` | `jpeg` | `webp`（默认 `png`）
- `width`：按宽度同比例缩放（可选，整数像素）
- `webp_quality`：WebP 质量 1–100（仅 `format=webp` 有效，默认 80）
- `webp_lossless`：WebP 无损模式（仅 `format=webp` 有效，默认 false）

示例：

```bash
# BN 图片：JPEG（默认质量 85），宽度 1000
curl -sS "http://localhost:3939/api/v1/image/bn?format=jpeg&width=1000" \
  -H "Content-Type: application/json" -d '{
    "n": 27,
    "auth": { "sessionToken": "..." },
    "theme": "Black",
    "embed_images": false
  }' > bn.jpg

# 单曲图片：WebP（有损，质量 82），宽度 1200
curl -sS "http://localhost:3939/api/v1/image/song?format=webp&width=1200&webp_quality=82" \
  -H "Content-Type: application/json" -d '{
    "song": "DEVIL",
    "auth": { "sessionToken": "..." },
    "theme": "White",
    "embed_images": false
  }' > song.webp

# 用户自报 BN：PNG（默认），不缩放
curl -sS "http://localhost:3939/api/v1/image/bn/user" \
  -H "Content-Type: application/json" -d '{
    "nickname": "Player",
    "theme": "Black",
    "records": [ { "song": "DEVIL", "difficulty": "IN", "score": 1000000, "acc": 100.0 } ]
  }' > bn.png
```

## 代码调用（统一编码入口）

在 `src/features/image/renderer.rs` 中新增了统一编码方法：

```rust
/// 统一的图片编码入口：根据 `format` 选择编码器，并返回字节与 Content-Type。
pub fn render_svg_unified(
    svg: String,
    is_user_generated: bool,
    format: Option<&str>,
    width: Option<u32>,
    webp_quality: Option<u8>,
    webp_lossless: Option<bool>,
) -> Result<(Vec<u8>, &'static str), AppError>
```

使用示例：

```rust
let (bytes, content_type) = renderer::render_svg_unified(
    svg_string,
    /* is_user_generated = */ false,
    Some("webp"),               // png/jpeg/webp
    Some(1200),                 // width
    Some(80),                   // webp_quality
    Some(false),                // webp_lossless
)?;
```

说明：
- JPEG 质量默认 85。
- WebP 默认 `webp_quality=80`、`webp_lossless=false`。
- `width` 为空时使用原始 SVG 尺寸。
