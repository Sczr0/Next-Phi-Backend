# ADR-0001：内部重构总纲领——外部契约不变、内部翻新、两阶段执行

- 日期：2026-09
- 状态：已接受
- 相关章节：docs/ARCHITECTURE.md §1/§2/§4

## 背景

现有 Phi-Backend 功能完善但存在已确诊病灶（统计接口慢、单库读写混池、SQLite 文件不收缩、
`save_submissions` 无限增长等，见总纲领 §6），且几乎没有任何工程纪律（无 deny.toml、
无工具链钉死、CI 只有及格线、无 Docker）。重写会破坏对外契约：站点前端消费 `/api/v2`，
`user_hash` 是全部业务表的连接键（换盐 = 身份事故，见 ISSUE 讨论记录），
`jwt_secret` 轮换会踢掉全部在线用户。DAU 100–200，正确路线不是蓝绿/多活，
而是"契约测试 + 秒级回滚 + 短暂维护窗"（r0semi-mp 同款取舍）。

## 决策

1. **只翻内部，不动外部**：C1（`/api/v2` 路由与 payload）、C2（`user_hash_salt`）、
   C3（`jwt_secret` 等密钥）、C4（SQLite 业务数据）钉死为不可变契约。
2. **架构采用六边形分层**（契约/核心/实现/组合根），依赖方向由 `tools/check-deps.py`
   物理强制；接口按"第二个实现出现再抽象"（论文原则 5）。
3. **执行分两阶段**：Phase 1 纯搬迁（行为不变、金标准测试全绿、可 bisect）；
   Phase 2 抽端口（contract 沉降 trait + 契约测试套件）。
4. **工程纪律立即上**：rust-toolchain.toml 钉死 1.98.0、deny.toml 许可白名单、
   CI 六道闸门（fmt/clippy/check-deps/check-adr/test/cargo-deny；semver-checks 随
   phi-contract 在 Phase 2 启用）、ADR 编号连续、AGENTS.md。
5. **不引入新基础设施**：SQLite + moka 正确；不加 PostgreSQL/Redis（选型雷达 Annex A）。

## 后果

- 正面：用户无感、统计不断、回滚秒级；内部可逐步翻新且每步可验证。
- 负面：接口抽象只能等第二个实现（短期无大接口）；Phase 1 搬运工作量大但机械。
- 风险与对策：换盐/换钥是最大风险——本决策用 C2/C3 冻结 + 轮换走"版本化 + 懒重绑定"（§5.1）兜底。
