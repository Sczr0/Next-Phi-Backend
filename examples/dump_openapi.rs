// 生成 OpenAPI JSON（无需启动服务），便于 SDK 代码生成
// 用法：cargo run --example dump_openapi

use utoipa::OpenApi;

fn main() {
    let openapi = phi_backend::openapi::ApiDoc::openapi();
    let json = serde_json::to_string_pretty(&openapi).expect("serialize openapi json");
    // 直接写入 UTF-8 文件，避免 PowerShell 重定向编码问题
    let _ = std::fs::create_dir_all("sdk");
    std::fs::write("sdk/openapi.json", json).expect("write openapi.json");
    println!("wrote sdk/openapi.json");
}
