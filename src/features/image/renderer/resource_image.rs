use std::path::Path;

/// renderer 本地资源索引支持的图片类型。
///
/// 这里保持既有资源扫描范围：只识别 `png` 与 `jpg`，不额外扩展到 `jpeg`。
#[derive(Clone, Copy)]
pub(super) enum LocalImageKind {
    Png,
    Jpeg,
}

impl LocalImageKind {
    /// 根据路径扩展名解析本地图片类型。
    pub(super) fn from_path(path: &Path) -> Option<Self> {
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("png") => Some(Self::Png),
            Some("jpg") => Some(Self::Jpeg),
            _ => None,
        }
    }

    /// 返回 data URI 使用的 MIME 类型。
    pub(super) fn mime_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
        }
    }
}
