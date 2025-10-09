# Phi-Backend 部署指南

本文档提供了 Phi-Backend 在不同平台上的部署指南，包括优雅退出和系统服务管理功能。

## 目录

- [系统要求](#系统要求)
- [快速开始](#快速开始)
- [Linux部署](#linux部署)
- [Windows部署](#windows部署)
- [macOS部署](#macos部署)
- [配置说明](#配置说明)
- [优雅退出](#优雅退出)
- [看门狗功能](#看门狗功能)
- [故障排除](#故障排除)

## 系统要求

### 最低要求
- **操作系统**: Linux (Ubuntu 20.04+, CentOS 8+), Windows 10+, macOS 11+
- **内存**: 512MB RAM
- **磁盘**: 1GB 可用空间
- **网络**: 端口 3939 可用

### 推荐配置
- **操作系统**: Linux (Ubuntu 22.04 LTS)
- **内存**: 2GB RAM
- **磁盘**: 5GB 可用空间
- **CPU**: 2核心

## 快速开始

### 1. 环境准备

```bash
# 安装 Rust (如果尚未安装)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# 克隆项目
git clone <repository-url>
cd phi-backend
```

### 2. 构建项目

```bash
# 生产构建
cargo build --release

# 运行测试
cargo test

# 验证构建
./target/release/phi-backend --version
```

### 3. 基本运行

```bash
# 直接运行
./target/release/phi-backend

# 或者指定配置文件
./target/release/phi-backend --config /path/to/config.toml
```

服务启动后访问：
- **API服务**: http://localhost:3939
- **API文档**: http://localhost:3939/docs
- **健康检查**: http://localhost:3939/health

## Linux部署

### 使用systemd服务（推荐）

#### 自动安装

```bash
# 运行安装脚本
sudo ./scripts/install-systemd-service.sh

# 使用管理脚本
sudo ./scripts/phi-backendctl status
```

#### 手动安装

1. **创建服务用户**
```bash
sudo useradd -r -s /bin/false -d /opt/phi-backend phi
```

2. **创建目录结构**
```bash
sudo mkdir -p /opt/phi-backend/{resources,info,logs}
sudo chown -R phi:phi /opt/phi-backend
```

3. **复制文件**
```bash
sudo cp target/release/phi-backend /opt/phi-backend/
sudo cp config.toml /opt/phi-backend/
sudo cp -r resources/* /opt/phi-backend/resources/
sudo cp -r info/* /opt/phi-backend/info/
sudo chown -R phi:phi /opt/phi-backend
sudo chmod +x /opt/phi-backend/phi-backend
```

4. **安装systemd服务**
```bash
sudo cp scripts/phi-backend.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable phi-backend
sudo systemctl start phi-backend
```

### Docker部署

```dockerfile
FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/phi-backend /usr/local/bin/
COPY --from=builder /app/config.toml /etc/phi-backend/
EXPOSE 3939
CMD ["phi-backend", "--config", "/etc/phi-backend/config.toml"]
```

```bash
# 构建和运行
docker build -t phi-backend .
docker run -d -p 3939:3939 -v $(pwd)/resources:/app/resources phi-backend
```

## Windows部署

### 使用批处理脚本

```cmd
# 构建项目
scripts\phi-backend.bat build

# 启动服务
scripts\phi-backend.bat start

# 检查状态
scripts\phi-backend.bat status
```

### 手动部署

1. **构建项目**
```cmd
cargo build --release
```

2. **配置环境**
```cmd
# 设置环境变量
set APP_RESOURCES_BASE_PATH=C:\phi-backend\resources
set APP_STATS_SQLITE_PATH=C:\phi-backend\resources\usage_stats.db
```

3. **运行服务**
```cmd
target\release\phi-backend.exe
```

### Windows服务

使用NSSM (Non-Sucking Service Manager):

```cmd
# 下载并安装NSSM
nssm install PhiBackend "C:\phi-backend\target\release\phi-backend.exe"
nssm set PhiBackend Arguments "--config C:\phi-backend\config.toml"
nssm set PhiBackend DisplayName "Phi Backend Service"
nssm start PhiBackend
```

## macOS部署

### 使用Homebrew

```bash
# 安装Rust
brew install rust

# 构建项目
cargo build --release

# 创建launchd服务
cp scripts/com.phi-backend.plist ~/Library/LaunchAgents/
launchctl load ~/Library/LaunchAgents/com.phi-backend.plist
```

### 手动运行

```bash
# 直接运行
./target/release/phi-backend

# 后台运行
nohup ./target/release/phi-backend > /tmp/phi-backend.log 2>&1 &
```

## 配置说明

### 基础配置

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
info_path = "./info"
```

### 优雅退出配置

```toml
[shutdown]
# 优雅退出超时时间（秒）
timeout_secs = 30
# 是否启用强制退出
force_quit = true
# 强制退出前的等待时间（秒）
force_delay_secs = 10

[shutdown.watchdog]
# 是否启用systemd看门狗（仅在Linux下生效）
enabled = false
# 看门狗超时时间（秒）
timeout_secs = 60
# 心跳间隔时间（秒）
interval_secs = 10
```

### 统计配置

```toml
[stats]
# 是否启用统计
enabled = true
storage = "sqlite"
sqlite_path = "./resources/usage_stats.db"
batch_size = 100
flush_interval_ms = 1000
retention_hot_days = 180

[stats.archive]
parquet = true
dir = "./resources/stats/v1/events"
compress = "zstd"
```

## 优雅退出

Phi-Backend支持完整的优雅退出机制：

### 支持的信号
- **Linux/macOS**: SIGINT (Ctrl+C), SIGTERM
- **Windows**: Ctrl+C
- **程序内部**: 调用API触发退出

### 退出流程
1. 接收到退出信号
2. 停止接受新的HTTP请求
3. 完成正在处理的请求
4. 刷新统计数据到数据库
5. 关闭文件和网络连接
6. 发送systemd停止信号（如果在systemd环境下）
7. 退出程序

### 配置参数

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `timeout_secs` | 30 | 优雅退出超时时间 |
| `force_quit` | true | 超时后是否强制退出 |
| `force_delay_secs` | 10 | 强制退出前等待时间 |

### 使用示例

```bash
# Linux/macOS: 发送SIGTERM信号
kill -TERM <pid>

# Linux/macOS: 发送SIGINT信号
kill -INT <pid>

# 使用systemd服务管理
sudo systemctl stop phi-backend

# 使用管理脚本
sudo ./scripts/phi-backendctl stop
```

## 看门狗功能

systemd看门狗功能可确保服务在异常情况下自动重启。

### 启用看门狗

```toml
[shutdown.watchdog]
enabled = true
timeout_secs = 60
interval_secs = 10
```

### systemd服务配置

```ini
[Service]
WatchdogSec=60
```

### 验证看门狗状态

```bash
# 检查服务状态
sudo systemctl status phi-backend

# 查看看门狗超时设置
systemctl show phi-backend --property=WatchdogUSec

# 查看日志
sudo journalctl -u phi-backend -f
```

## 服务管理

### Linux (systemd)

```bash
# 使用管理脚本
sudo ./scripts/phi-backendctl status
sudo ./scripts/phi-backendctl start
sudo ./scripts/phi-backendctl stop
sudo ./scripts/phi-backendctl restart
sudo ./scripts/phi-backendctl logs -f

# 直接使用systemctl
sudo systemctl status phi-backend
sudo systemctl start phi-backend
sudo systemctl stop phi-backend
sudo systemctl restart phi-backend
sudo journalctl -u phi-backend -f
```

### Windows

```cmd
# 使用批处理脚本
scripts\phi-backend.bat status
scripts\phi-backend.bat start
scripts\phi-backend.bat stop
scripts\phi-backend.bat restart

# 使用NSSM (如果已安装为服务)
nssm status PhiBackend
nssm start PhiBackend
nssm stop PhiBackend
nssm restart PhiBackend
```

### 健康检查

```bash
# Linux/macOS
curl http://localhost:3939/health

# Windows
curl http://localhost:3939/health

# 使用管理脚本
sudo ./scripts/phi-backendctl health
```

## 故障排除

### 常见问题

#### 1. 端口被占用
```bash
# 查看端口占用
sudo netstat -tlnp | grep 3939

# 杀死占用进程
sudo kill -9 <pid>
```

#### 2. 权限问题
```bash
# 检查文件权限
ls -la /opt/phi-backend/

# 修复权限
sudo chown -R phi:phi /opt/phi-backend/
sudo chmod +x /opt/phi-backend/phi-backend
```

#### 3. 服务启动失败
```bash
# 查看详���日志
sudo journalctl -u phi-backend -n 50

# 检查配置文件
sudo -u phi /opt/phi-backend/phi-backend --config /opt/phi-backend/config.toml --check
```

#### 4. 统计数据库问题
```bash
# 检查SQLite数据库
sqlite3 /opt/phi-backend/resources/usage_stats.db ".schema"

# 检查权限
ls -la /opt/phi-backend/resources/
```

### 日志级别

在配置文件中设置日志级别：

```toml
[logging]
level = "debug"  # trace, debug, info, warn, error
format = "full"  # full, compact, pretty, json
```

或者通过环境变量：

```bash
export RUST_LOG=debug
./target/release/phi-backend
```

### 性能监控

```bash
# 查看进程状态
ps aux | grep phi-backend

# 查看资源使用
htop
iotop

# 网络连接
netstat -tlnp | grep 3939
ss -tlnp | grep 3939
```

## 升级指南

### 滚动升级

```bash
# 1. 备份配置和数据
sudo cp /opt/phi-backend/config.toml /opt/phi-backend/config.toml.bak
sudo cp -r /opt/phi-backend/resources /opt/phi-backend/resources.bak

# 2. 停止服务
sudo systemctl stop phi-backend

# 3. 更新二进制文件
sudo cp target/release/phi-backend /opt/phi-backend/
sudo chown phi:phi /opt/phi-backend/phi-backend
sudo chmod +x /opt/phi-backend/phi-backend

# 4. 重启服务
sudo systemctl start phi-backend

# 5. 验证升级
sudo systemctl status phi-backend
curl http://localhost:3939/health
```

### 配置迁移

新版本的配置文件可能有新增字段，建议：

1. 备份现有配置
2. 查看新的默认配置文件
3. 合并配置更改
4. 测试配置文件语法

## 安全建议

1. **防火墙配置**
```bash
# 限制访问端口
sudo ufw allow 3939/tcp
sudo ufw enable
```

2. **用户权限**
- 使用专用用户运行服务
- 限制文件系统权限
- 定期更新系统和依赖

3. **SSL/TLS**
- 使用反向代理 (nginx/apache)
- 配置HTTPS证书
- 强制HTTPS重定向

4. **监控和告警**
- 设置日志监控
- 配置服务状态检查
- 设置资源使用告警

## 支持

如遇到问题，请：

1. 查看日志文件
2. 检查配置文件
3. 运行健康检查
4. 提交Issue到项目仓库

---

**注意**: 本文档会随着项目更新而持续更新。建议定期查看最新版本的部署指南。