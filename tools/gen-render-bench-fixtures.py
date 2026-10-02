#!/usr/bin/env python3
"""生成 impl-render 渲染基准测试的曲绘 fixture。

CodSpeed/CI 检出后没有 `resources/ill/`（.gitignore 排除的曲绘仓库），
但 B27 渲染的成本大头正是 27 张曲绘的解码与合成。为了让基准测试覆盖
这条生产路径，从本机真实曲绘中确定性降采样出一组小型 fixture 提交入库：

- 封面：`resources/ill/ill/` 前 10 张（按文件名排序）→ 512x270 JPEG q85
- 背景：`resources/ill/illBlur/` 第 1 张 → 原样复制（256x135，本就只有 ~60KB）

输出目录（git 跟踪，不受 `/resources/**/ill/` 忽略规则影响）：
    crates/impl-render/benches/fixtures/resources/ill/{ill,illBlur}/

运行前提：本机存在真实曲绘（见 config.toml / 曲绘仓库同步）。
重复运行幂等：输出按固定顺序覆盖。
"""

from __future__ import annotations

import shutil
import sys
from pathlib import Path

from PIL import Image

REPO_ROOT = Path(__file__).resolve().parent.parent
SRC_ILL = REPO_ROOT / "resources" / "ill" / "ill"
SRC_BLUR = REPO_ROOT / "resources" / "ill" / "illBlur"
DST_ROOT = REPO_ROOT / "crates" / "impl-render" / "benches" / "fixtures" / "resources" / "ill"

COVER_COUNT = 10
COVER_SIZE = (512, 270)  # 真实曲绘为 2048x1080；1/4 线性缩放，1/16 像素量
JPEG_QUALITY = 85


def main() -> int:
    if not SRC_ILL.is_dir():
        print(f"错误：找不到真实曲绘目录 {SRC_ILL}", file=sys.stderr)
        print("请先在本机同步曲绘仓库，或换一台有曲绘的机器运行。", file=sys.stderr)
        return 1

    dst_ill = DST_ROOT / "ill"
    dst_blur = DST_ROOT / "illBlur"
    dst_ill.mkdir(parents=True, exist_ok=True)
    dst_blur.mkdir(parents=True, exist_ok=True)

    # 封面：排序后取前 N，保证可复现
    covers = sorted(p for p in SRC_ILL.iterdir() if p.suffix == ".png")[:COVER_COUNT]
    if len(covers) < COVER_COUNT:
        print(f"错误：真实曲绘不足 {COVER_COUNT} 张（当前 {len(covers)}）", file=sys.stderr)
        return 1

    total = 0
    for src in covers:
        dst = dst_ill / (src.stem + ".jpg")
        with Image.open(src) as img:
            img = img.convert("RGB").resize(COVER_SIZE, Image.LANCZOS)
            img.save(dst, "JPEG", quality=JPEG_QUALITY, optimize=True)
        total += dst.stat().st_size
        print(f"封面  {dst.name:60s} {dst.stat().st_size / 1024:7.1f} KB")

    # 背景：原样复制第一张（illBlur 本身就是 256x135 的小图）
    blur = sorted(p for p in SRC_BLUR.iterdir() if p.suffix == ".png")[0]
    dst = dst_blur / blur.name
    shutil.copyfile(blur, dst)
    total += dst.stat().st_size
    print(f"背景  {dst.name:60s} {dst.stat().st_size / 1024:7.1f} KB")

    print(f"\n共 {COVER_COUNT + 1} 个文件，合计 {total / 1024:.0f} KB -> {DST_ROOT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
