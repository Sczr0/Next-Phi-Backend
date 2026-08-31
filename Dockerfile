# syntax=docker/dockerfile:1
# Phi-Backend 运行时镜像（D-01，Charter §8.1）。
# 策略：多阶段——builder 用 clux/muslrust（自带 musl 工具链，git2 vendored-openssl
# 无系统依赖问题；tag 与 rust-toolchain.toml 对齐 1.98.0）；
# 产物 = release-dist（strip + fat LTO + codegen-units=1）静态单一二进制。
# 运行时 alpine 只带二进制 + 配置模板 + 系统字体（fontdb::load_system_fonts，
# B27/单曲图渲染中文曲名必需——CJK 字体不可省）。

FROM clux/muslrust:1.98.0-stable AS builder
WORKDIR /src
# 依赖层优先（利用层缓存）：先复制清单，依赖编译命中缓存
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
# 源码层
COPY src ./src
RUN cargo build --profile release-dist --target x86_64-unknown-linux-musl --bin phi-backend

FROM alpine:3.20
WORKDIR /app
# 系统字体（fontdb::load_system_fonts 扫描 /usr/share/fonts；CJK 必需）
RUN apk add --no-cache fontconfig font-noto-cjk font-noto
# 运行时仅二进制 + 配置模板；资源（曲绘/info/SQLite）经卷挂载（见 docker-compose.yml）
COPY --from=builder /src/target/x86_64-unknown-linux-musl/release-dist/phi-backend /app/phi-backend
COPY config.example.toml /app/config.example.toml
# 优雅停机（shutdown.rs 捕获 SIGTERM 维护广播 + 宽限窗口；Charter §8.1）
EXPOSE 3939
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
  CMD wget -qO- http://127.0.0.1:3939/health || exit 1
STOPSIGNAL SIGTERM
CMD ["/app/phi-backend"]
