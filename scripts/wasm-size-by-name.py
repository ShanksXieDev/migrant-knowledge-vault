#!/usr/bin/env python3
"""按函数名归因 wasm 体积。

背景：release wasm 的 DWARF `.debug_info` 在 wasm 目标下把 `DW_AT_low_pc`
解释成函数下标的做法并不可靠（实测归因结果 99.9% 落到 unknown），
而 `name` 自定义段里保留了**函数名到下标的映射**。因此这里直接：
  1. 解析 `name` 段 → 函数下标 → 名字；
  2. 解析 `code` 段 → 每个函数体的字节数；
  3. 按「crate / 模块」关键词聚合。

用途：wasm 体积门禁逼近时定位真正的体积来源，而不是靠猜。

用法：
    python scripts/wasm-size-by-name.py <wasm> [--top N]
"""
import os
import re
import sys
import collections

SECTION_CODE = 10


def uleb(data, i):
    r = 0
    s = 0
    while True:
        b = data[i]
        i += 1
        r |= (b & 0x7F) << s
        if not (b & 0x80):
            break
        s += 7
    return r, i


def read_sections(data):
    assert data[:4] == b"\x00asm", "不是合法的 wasm 文件"
    i = 8
    out = []
    while i < len(data):
        sid = data[i]
        i += 1
        size, i = uleb(data, i)
        out.append((sid, i, size))
        i += size
    return out


def parse_name_section(data, start, size):
    """返回 {函数下标: 名字}。"""
    end = start + size
    i = start
    # 段名
    nlen, i = uleb(data, i)
    i += nlen
    names = {}
    while i < end:
        sub_id = data[i]
        i += 1
        sub_size, i = uleb(data, i)
        sub_end = i + sub_size
        if sub_id == 1:  # function names
            count, j = uleb(data, i)
            for _ in range(count):
                idx, j = uleb(data, j)
                ln, j = uleb(data, j)
                names[idx] = data[j:j + ln].decode("utf-8", "replace")
                j += ln
        i = sub_end
    return names


def parse_code_sizes(data, start, size):
    """返回 [每个函数体的字节数]（下标即函数序号）。"""
    i = start
    count, i = uleb(data, i)
    sizes = []
    for _ in range(count):
        body, i = uleb(data, i)
        sizes.append(body)
        i += body
    return sizes


# 归因关键词：按出现顺序匹配，命中即计入该桶。
BUCKETS = [
    ("mkv_core::regex", r"regex"),
    ("mkv_core::office_locator", r"office_locator"),
    ("mkv_core::diagram", r"diagram"),
    ("mkv_core::query", r"query"),
    ("mkv_core::tags", r"tags"),
    ("mkv_core::refs", r"refs"),
    ("mkv_core::locate", r"locate"),
    ("mkv_core::plugin", r"plugin"),
    ("mkv_core::backend", r"backend"),
    ("mkv_core::prebuilt", r"prebuilt"),
    ("mkv_core::editor", r"editor"),
    ("mkv_ui", r"mkv_ui"),
    ("mkv_web", r"mkv_web"),
    ("serde_json", r"serde_json"),
    ("serde", r"serde"),
    ("alloc", r"alloc"),
    ("core", r"core|corec"),
    ("std", r"std"),
    ("wasm_bindgen", r"wasm_bindgen|wbind"),
]


def bucket_of(name):
    for label, pat in BUCKETS:
        if re.search(pat, name):
            return label
    return "其他"


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    path = sys.argv[1]
    top = 30
    if "--top" in sys.argv:
        top = int(sys.argv[sys.argv.index("--top") + 1])

    data = open(path, "rb").read()
    sections = read_sections(data)
    names = {}
    code = []
    for sid, start, size in sections:
        if sid == 0:
            nlen, j = uleb(data, start)
            sec_name = data[start + 0:start + 0]  # 占位，下面重新取
            sec_name = data[start:start]
            # 重新解析段名
            k = start
            ln, k = uleb(data, k)
            sec_name = data[k:k + ln].decode("utf-8", "replace")
            if sec_name == "name":
                names = parse_name_section(data, start, size)
        elif sid == SECTION_CODE:
            code = parse_code_sizes(data, start, size)

    if not code:
        print("未找到 code 段")
        return 1

    total = sum(code)
    buckets = collections.Counter()
    unknown = 0
    per_func = []
    for idx, size in enumerate(code):
        name = names.get(idx)
        if name is None:
            unknown += size
            per_func.append((size, f"<未命名 #{idx}>"))
            continue
        buckets[bucket_of(name)] += size
        per_func.append((size, name))

    print(f"文件        : {path}")
    print(f"文件大小    : {os.path.getsize(path):,} bytes")
    print(f"代码段总计  : {total:,} bytes / {len(code)} 个函数")
    print(f"未命名函数  : {unknown:,} bytes")
    print()
    print("== 按模块归因 ==")
    for label, size in buckets.most_common():
        print(f"{size:>10,}  {size / total * 100:5.1f}%  {label}")
    print()
    print(f"== 最大的 {top} 个函数 ==")
    for size, name in sorted(per_func, reverse=True)[:top]:
        short = name if len(name) <= 110 else name[:107] + "..."
        print(f"{size:>8,}  {short}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
