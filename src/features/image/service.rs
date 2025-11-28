// 预留的服务层封装（当前处理逻辑直接在 handler 中实现）
pub struct ImageService;

impl Default for ImageService {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageService {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self
    }
}
