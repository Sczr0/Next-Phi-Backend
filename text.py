#!/usr/bin/env python3
# 判断 PNG 首像素是否为隐式水印标记 (1,2,3,255)
# 依赖: Pillow (pip install pillow)

import sys
from PIL import Image


def is_user_generated(png_path: str) -> bool:
    im = Image.open(png_path).convert("RGBA")
    r, g, b, a = im.getpixel((0, 0))
    return (r, g, b, a) == (1, 2, 3, 255)


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: check_user_generated.py <image.png>")
        sys.exit(2)
    ok = is_user_generated(sys.argv[1])
    print("user_generated:", "true" if ok else "false")
    sys.exit(0 if ok else 1)
