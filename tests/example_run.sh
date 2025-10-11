#!/bin/bash
# B27 性能测试运行脚本 (Unix/Linux/macOS)
#
# 使用方法:
# 1. 编辑此文件，将 YOUR_SESSION_TOKEN_HERE 替换为你的真实 session token
# 2. 赋予执行权限: chmod +x tests/example_run.sh
# 3. 运行: ./tests/example_run.sh

# 设置 Session Token
export PHI_SESSION_TOKEN="YOUR_SESSION_TOKEN_HERE"

# 检查 token 是否已设置
if [ "$PHI_SESSION_TOKEN" = "YOUR_SESSION_TOKEN_HERE" ]; then
    echo -e "\033[0;31m错误: 请先在脚本中设置你的 Session Token!\033[0m"
    echo -e "\033[0;33m打开 tests/example_run.sh 文件，将 YOUR_SESSION_TOKEN_HERE 替换为实际的 token\033[0m"
    exit 1
fi

echo -e "\033[0;32m开始运行 B27 性能测试...\033[0m"
echo -e "\033[0;36mSession Token: ${PHI_SESSION_TOKEN:0:10}...\033[0m"

# 运行测试
cargo test --test b27_performance_test -- --nocapture --ignored

# 检查测试结果
if [ $? -eq 0 ]; then
    echo -e "\n\033[0;32m测试成功完成!\033[0m"
    echo -e "\033[0;36m查看输出文件:\033[0m"
    echo -e "  - tests/output/b27.png          (生成的图片)"
    echo -e "  - tests/output/performance.txt  (性能报告)"
    
    # 如果存在火焰图，也显示
    if [ -f "tests/output/flamegraph.svg" ]; then
        echo -e "  - tests/output/flamegraph.svg   (火焰图)"
    fi
else
    echo -e "\n\033[0;31m测试失败!\033[0m"
    echo -e "\033[0;33m请检查错误信息并确保:\033[0m"
    echo -e "  1. Session Token 正确且未过期"
    echo -e "  2. 网络连接正常"
    echo -e "  3. config.toml 配置正确"
    echo -e "  4. 资源文件存在 (info/difficulty.csv 等)"
fi
