# B27 性能测试运行脚本 (Windows PowerShell)
#
# 使用方法:
# 1. 编辑此文件，将 YOUR_SESSION_TOKEN_HERE 替换为你的真实 session token
# 2. 运行: .\tests\example_run.ps1

# 设置 Session Token
$env:PHI_SESSION_TOKEN = "YOUR_SESSION_TOKEN_HERE"

# 检查 token 是否已设置
if ($env:PHI_SESSION_TOKEN -eq "YOUR_SESSION_TOKEN_HERE") {
    Write-Host "错误: 请先在脚本中设置你的 Session Token!" -ForegroundColor Red
    Write-Host "打开 tests\example_run.ps1 文件，将 YOUR_SESSION_TOKEN_HERE 替换为实际的 token" -ForegroundColor Yellow
    exit 1
}

Write-Host "开始运行 B27 性能测试..." -ForegroundColor Green
Write-Host "Session Token: $($env:PHI_SESSION_TOKEN.Substring(0, [Math]::Min(10, $env:PHI_SESSION_TOKEN.Length)))..." -ForegroundColor Cyan

# 运行测试
cargo test --test b27_performance_test -- --nocapture --ignored

# 检查测试结果
if ($LASTEXITCODE -eq 0) {
    Write-Host "`n测试成功完成!" -ForegroundColor Green
    Write-Host "查看输出文件:" -ForegroundColor Cyan
    Write-Host "  - tests\output\b27.png          (生成的图片)" -ForegroundColor White
    Write-Host "  - tests\output\performance.txt  (性能报告)" -ForegroundColor White
    
    # 如果存在火焰图（Unix 系统），也显示
    if (Test-Path "tests\output\flamegraph.svg") {
        Write-Host "  - tests\output\flamegraph.svg   (火焰图)" -ForegroundColor White
    }
} else {
    Write-Host "`n测试失败!" -ForegroundColor Red
    Write-Host "请检查错误信息并确保:" -ForegroundColor Yellow
    Write-Host "  1. Session Token 正确且未过期" -ForegroundColor White
    Write-Host "  2. 网络连接正常" -ForegroundColor White
    Write-Host "  3. config.toml 配置正确" -ForegroundColor White
    Write-Host "  4. 资源文件存在 (info/difficulty.csv 等)" -ForegroundColor White
}
