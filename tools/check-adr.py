#!/usr/bin/env python3
"""ADR 编号连续性检查（Charter §7.6 / ISSUE-0002 防线，CI 第 3b 道闸门）。

规则：docs/adr/ 下文件名形如 `NNNN-<kebab>.md`，编号必须从 0001 起连续无缺。
docs/adr/ 不存在或为空时放行（提示先立第一条 ADR）。
"""
from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
ADR_DIR = ROOT / "docs" / "adr"
PATTERN = re.compile(r"^(\d{4})-[\w\-]+\.md$")


def main() -> int:
    if not ADR_DIR.exists():
        print("[check-adr] WARN: docs/adr/ 尚不存在（新决策请先立 ADR-0001）")
        return 0

    files = sorted(p for p in ADR_DIR.iterdir() if p.is_file() and PATTERN.match(p.name))
    if not files:
        print("[check-adr] WARN: docs/adr/ 为空（新决策请先立 ADR-0001）")
        return 0

    nums = [int(PATTERN.match(p.name).group(1)) for p in files]
    expected = list(range(1, len(nums) + 1))
    if nums != expected:
        missing = sorted(set(expected) - set(nums))
        dup = sorted({n for n in nums if nums.count(n) > 1})
        print(f"[check-adr] FAIL: 编号不连续。期望 0001-{len(nums):04d}；缺 {missing}；重复 {dup}")
        return 1

    print(f"[check-adr] PASS: ADR 编号连续（0001-{len(nums):04d}，共 {len(nums)} 条）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
