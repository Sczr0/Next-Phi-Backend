#!/usr/bin/env python3
"""依赖方向物理闸门（Charter §3.3，CI 第 3 道闸门，Phase 2 后启用）。

原理：直接解析 workspace 内各 crate 的 Cargo.toml（无网络、无 cargo metadata 调用），
提取内部依赖边，与 ALLOW 白名单比对。内部 crate 之间任何未白名单的边 -> 退出码 1。

当前（Phase 1 前）：phi-backend <-> phi-save-codec。
Phase 2 后示例（见 docs/ARCHITECTURE.md §3.3）：
  phi-contract: [], phi-core: [phi-contract], impl-*: [phi-contract],
  phi-server: [phi-contract, phi-core, impl-*]
新增内部 crate 时必须先更新本文件，否则 CI 红。
"""
from __future__ import annotations

import pathlib
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent

# 内部 crate -> 允许的 normal 依赖（内部 crate 名）
ALLOW = {
    "phi-common": [],
    "phi-save-codec": [],
    "phi-contract": ["phi-save-codec"],
    # Phase 1 临时：phi-http 位于根 crate 与契约之间（AppError 的 CodecError 映射）。
    # Phase 2 组合根成型后并入 phi-server 网关层。
    "phi-http": ["phi-contract", "phi-save-codec"],
    # 存储实现：唯一 sqlx 所在；只认识契约与错误面（Charter §3.3）。
    "impl-storage": ["phi-contract", "phi-http"],
    # RKS 引擎：纯计算，只认识契约（被渲染层与业务层共享）。
    "impl-rks": ["phi-contract"],
    # 渲染实现：重 CPU 依赖隔离；认识契约/引擎/配置/错误面。
    "impl-render": ["impl-rks", "phi-common", "phi-contract", "phi-http"],
    "phi-backend": [
        "impl-render",
        "impl-rks",
        "impl-storage",
        "phi-common",
        "phi-contract",
        "phi-http",
        "phi-save-codec",
    ],
}
# 允许的 dev 依赖（内部 crate 名；如未来 impl-* -> phi-contract 的契约测试链）
ALLOW_DEV = {
    "phi-backend": [],
    "phi-common": [],
    "phi-contract": [],
    "phi-http": [],
    "impl-render": [],
    "impl-rks": [],
    "impl-storage": [],
    "phi-save-codec": [],
}


def parse_members(root: pathlib.Path) -> list[pathlib.Path]:
    cargo = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    members: list[pathlib.Path] = []
    for m in cargo.get("workspace", {}).get("members", []):
        if "*" in m:
            base = root / m.rstrip("/*")
            members.extend(sorted(p.parent for p in base.glob("*/Cargo.toml")))
        else:
            members.append(root / m)
    return members


def crate_name(manifest: pathlib.Path) -> str:
    cargo = tomllib.loads(manifest.read_text(encoding="utf-8"))
    return cargo.get("package", {}).get("name") or ""


def internal_edges(manifest: pathlib.Path, internal_names: set[str]) -> list[tuple[str, str]]:
    """返回 [(kind, dep_name)]，kind ∈ normal/dev/build。"""
    cargo = tomllib.loads(manifest.read_text(encoding="utf-8"))
    ws_deps = cargo.get("workspace", {}).get("dependencies", {}) or {}
    # 收集所有可能含 dependencies 的节：顶层 + [target.*.*]
    sections: list[tuple[str, str]] = []
    for key, kind in (("dependencies", "normal"), ("dev-dependencies", "dev"), ("build-dependencies", "build")):
        sections.append((f"{key}", kind))
    for key, val in cargo.items():
        if key.endswith(".dependencies") and isinstance(val, dict):
            sections.append((key, "normal"))

    edges: list[tuple[str, str]] = []
    for section, kind in sections:
        deps = cargo.get(section) or {}
        for name, spec in deps.items():
            resolved = None
            if isinstance(spec, dict):
                if spec.get("path"):
                    resolved = ("path", spec["path"])
                elif spec.get("workspace") and name in ws_deps:
                    wd = ws_deps[name]
                    resolved = ("path", wd["path"]) if isinstance(wd, dict) and wd.get("path") else None
            if resolved is not None:
                # path 边：目标若是内部 crate（名字匹配）则记录
                if name in internal_names:
                    edges.append((kind, name))
            elif name in internal_names:
                # 按版本引用内部 crate 名：也视为内部边（正常构建依赖应为 path）
                edges.append((kind, name))
    return edges


def main() -> int:
    internal_names = set()
    manifests: dict[str, pathlib.Path] = {}
    for m in parse_members(ROOT):
        name = crate_name(m / "Cargo.toml")
        if name:
            internal_names.add(name)
            manifests[name] = m

    violations: list[str] = []
    for name, manifest in sorted(manifests.items()):
        for kind, dep in internal_edges(manifest / "Cargo.toml", internal_names):
            allowed = ALLOW_DEV.get(name, []) if kind == "dev" else ALLOW.get(name, [])
            if dep not in allowed:
                violations.append(f"{name} ({kind}) -> {dep}")

    if violations:
        print("[check-deps] FAIL: 未白名单的内部依赖边：")
        for v in sorted(violations):
            print(f"  - {v}")
        print("请更新 ALLOW/ALLOW_DEV（新增内部 crate 时先更新本文件）")
        return 1
    print(f"[check-deps] PASS: 内部 crate = {sorted(internal_names)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
