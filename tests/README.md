# B27 性能测试使用说明

## 功能说明

这个测试用于分析 B27（Best27）图片生成的完整性能，包括：
- 存档获取与解密
- 成绩解析与 RKS 计算
- 推分 ACC 计算
- SVG 生成
- PNG 渲染

测试会生成：
1. **b27.png** - Best27 成绩图片
2. **performance.txt** - 详细的性能统计报告
3. **flamegraph.svg** - 完整流程的性能火焰图（仅 Unix/Linux/macOS）

## 使用方法

### 1. 准备环境

确保已安装 Rust 和 Cargo：
```bash
rustc --version
cargo --version
```

### 2. 设置 Session Token

设置环境变量 `PHI_SESSION_TOKEN` 为你的 Phigros 会话令牌：

**Windows (PowerShell):**
```powershell
$env:PHI_SESSION_TOKEN="你的session_token"
```

**Windows (CMD):**
```cmd
set PHI_SESSION_TOKEN=你的session_token
```

**Linux/macOS:**
```bash
export PHI_SESSION_TOKEN="你的session_token"
```

### 3. 运行测试

```bash
cargo test --test b27_performance_test -- --nocapture --ignored
```

参数说明：
- `--test b27_performance_test` - 指定运行 b27 性能测试
- `--nocapture` - 显示测试输出（包括性能统计）
- `--ignored` - 运行标记为 `#[ignore]` 的测试

### 4. 查看结果

测试完成后，在 `tests/output/` 目录下可以找到：

```
tests/output/
├── b27.png            # 生成的 Best27 图片
├── performance.txt    # 性能统计报告
└── flamegraph.svg     # 性能火焰图（仅 Unix/Linux/macOS）
```

## 输出示例

运行测试时会显示各阶段的耗时：

```
========================================
B27 图片生成性能测试
========================================

阶段 1: 初始化配置...
  耗时: 2.5ms

阶段 2: 加载资源文件...
  加载了 XXX 首歌曲
  耗时: 45ms

阶段 3: 获取并解密存档...
  解析了 XXX 首歌曲的成绩
  耗时: 850ms

阶段 4: 计算 RKS 并排序...
  总成绩数: XXX
  Best27 平均 RKS: 15.XXXX
  耗时: 12ms

阶段 5: 计算推分 ACC...
  计算了 XX 首歌曲的推分 ACC
  耗时: 8ms

阶段 6: 生成统计信息...
  玩家 RKS: 15.XXXX
  AP Top3 平均: 16.XXXX
  耗时: 1ms

阶段 7: 渲染 SVG...
  SVG 大小: XXXXX bytes
  耗时: 120ms

阶段 8: 渲染 PNG...
  PNG 大小: XXXXX bytes
  耗时: 450ms

========================================
测试完成!
  总耗时: 1.5s
  输出文件: tests\output\b27.png
========================================

生成火焰图...
火焰图已保存到: tests\output\flamegraph.svg

性能分析完成!
```

## 性能报告分析

### 文本报告

打开 `tests/output/performance.txt` 可以查看每个阶段的详细耗时：

```
B27 图片生成性能测试报告
==================================================
测试时间: 2024-01-15 10:30:45
操作系统: windows
架构: x86_64
==================================================

阶段 1: 初始化配置 - 2.5ms
阶段 2: 加载资源文件 - 45ms (加载 XXX 首歌曲)
阶段 3: 获取并解密存档 - 850ms (解析 XXX 首歌曲)
阶段 4: 计算 RKS 并排序 - 12ms (总成绩 XXX, Best27 平均 XX.XXXX)
阶段 5: 计算推分 ACC - 8ms (计算 XX 首)
阶段 6: 生成统计信息 - 1ms (玩家 RKS XX.XXXX, AP Top3 平均 XX.XXXX)
阶段 7: 渲染 SVG - 120ms (大小 XXXXX bytes)
阶段 8: 渲染 PNG - 450ms (大小 XXXXX bytes)

==================================================
总耗时: 1.5s
输出图片: tests\output\b27.png
```

### 火焰图分析（仅 Unix/Linux/macOS）

打开 `tests/output/flamegraph.svg` 可以查看详细的性能分析：

1. **横轴** - 表示CPU时间占比（宽度越宽，占用时间越多）
2. **纵轴** - 表示函数调用栈（从下到上是调用链）
3. **颜色** - 随机分配，便于区分不同的调用栈

### 关键性能指标

通常需要关注的部分：
- 存档下载和解密（网络IO + 加密计算）
- RKS 计算和排序（算法复杂度）
- SVG 生成（字符串拼接和模板渲染）
- PNG 渲染（resvg + tiny-skia）

## 故障排除

### 错误：PHI_SESSION_TOKEN 未设置

```
thread 'test_b27_generation_with_flamegraph' panicked at '请设置环境变量 PHI_SESSION_TOKEN'
```

**解决方法：** 设置环境变量后重试

### 错误：配置初始化失败

```
thread 'test_b27_generation_with_flamegraph' panicked at '配置初始化失败: ...'
```

**解决方法：** 检查 `config.toml` 是否存在且格式正确

### 错误：加载资源文件失败

```
thread 'test_b27_generation_with_flamegraph' panicked at '加载 difficulty.csv 失败: ...'
```

**解决方法：** 确保 `info/difficulty.csv` 等资源文件存在

## 性能优化建议

根据火焰图分析结果，可能的优化方向：

1. **存档获取** - 使用缓存减少重复请求
2. **解密计算** - 考虑使用更快的加密库或硬件加速
3. **RKS计算** - 使用并行计算处理大量成绩
4. **SVG生成** - 优化字符串拼接，使用高效的模板引擎
5. **PNG渲染** - 调整渲染质量参数，权衡质量与速度

## 附加说明

- 测试需要网络连接（获取存档）
- Session Token 需要有效且未过期
- **Windows 系统**：会生成详细的性能统计报告（performance.txt）
- **Unix/Linux/macOS 系统**：除性能报告外，还会生成火焰图（flamegraph.svg）
- 火焰图采样频率为 1000Hz，可在代码中调整
