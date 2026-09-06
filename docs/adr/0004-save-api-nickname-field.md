# ADR-0004：/save 响应新增昵称字段（C1 additive 变更——已接受）

- 日期：2026-09
- 状态：**已接受**
- 相关章节：docs/ARCHITECTURE.md §契约（C1）；AGENTS.md 红线 C1

## 背景

`/save`（`/api/v2/save`）返回解析后的存档，其中 `data.user` 来自存档 `user` 块，
仅含 `showPlayerId / selfIntro / avatar / background`——**Phigros 存档格式本身不含
游戏昵称**（`summaryParsed`、`gameProgress` 亦无）。昵称是 LeanCloud 账号侧数据，
只能携会话令牌请求 `users/me` 获得（需 X-LC-Id/X-LC-Key，消费方无法自取）。
图片链路（`/image/song`、`/image/bn`）早已用同一机制解析昵称（image/handler/nickname.rs），
但 `/save` 从未返回，消费方拿不到玩家昵称。

按 AGENTS.md 纪律，`/api/v2/*` 响应 payload 属于不可变契约 C1，任何变更（含
additive）须先立本 ADR。已核实 `tests/` 目录无任何测试钉定 `/save` 响应字段集合
（api_contract_v2.rs 仅断言错误响应与请求序列化），金标准回归网不受影响。

## 决策

1. **字段形态**：响应顶层新增 `nickname: Option<String>`，两个响应形状一致：
   - `{ data: {...}, nickname }`（默认）；
   - `{ save: {...}, rks: {...}, gradeCounts: {...}, nickname }`（calculate_rks=true）。
   序列化用 `skip_serializing_if = "Option::is_none"`——**解析不出昵称时字段整体
   省略**，无令牌路径的响应字节与现状完全一致。昵称不属于存档格式，故不注入
   `data.user`，`ParsedSave` 不动。
2. **解析范围**：三种同质会话令牌路径——body 官方 `sessionToken`、Bearer 内嵌
   凭证（合并进 payload 后统一读取）、`externalCredentials.sessiontoken`（其存档
   流程本就走官方接口）。platform/apiUserId 等无令牌外部凭证不解析，字段省略。
3. **默认返回，非 opt-in**：owner 拍板（2026-09）。消费方零改动。
4. **新鲜度与缓存**：
   - 昵称解析带进程内 moka 缓存（key = 会话令牌 SHA-256 前 16 字节 hex，
     TTL 600s，容量 1024，常量不设配置面），上游 `users/me` 调用频率被限界。
   - 默认响应体按 `user_hash:updatedAt:version` 整体缓存（save 缓存，TTL 默认
     120s）：昵称在缓存 miss 时一并烤入，**陈旧窗口 ≤ save 缓存 TTL**；
     calculate_rks 路径每请求新鲜解析（昵称缓存兜频率）。用户改名最迟约
     2 分钟反映——可接受，不做跨缓存失效机制。
5. **失败语义**：`users/me` 失败/超时（总超时 2s，`client_default` 仅 10s 连接
   超时故必须显式包裹）一律降级为字段省略，**绝不使 /save 失败**；解析与存档
   获取并发执行（tokio join），plain 缓存 miss 路径零额外延迟。

## 后果

- 正面：消费方从 `/save` 直接获得昵称，无需自建 LeanCloud 凭证；图片链路复用
  共享实现并获得缓存收益（原先每次渲染都打 users/me）。
- 负面：有令牌路径的响应体多一个字段（严格 JSON 解析的客户端理论上可见）；
  缓存 miss 路径多一次上游调用（有昵称缓存限界）。无令牌路径零变化。
- 实施状态：与本 ADR 同提交落地（save/nickname.rs 共享模块、handler 接线、
  Swagger 更新、单测）。
