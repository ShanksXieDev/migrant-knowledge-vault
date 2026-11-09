#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""wasm 段构成归因：把最终产物拆成「代码段 / 数据段 / 其他」。

# 为什么需要这个工具

体积门禁只有一个总量阈值（800,000 B）。它能回答「过没过」，但回答不了
「**为什么**」。2026-11-09 的专家团评审实测发现：wasm 侧的 diff 引擎与
`office_locator` 已被完全 DCE（`name` section 命中数为 0），
真正随知识库线性增长的是**内嵌的索引产物**（`include_str!` 编译进 wasm）——
它落在 **data 段**里，与业务代码的增长来源完全不同。

只盯总量门禁，会把「业务代码写多了」和「知识库变大了」两种完全不同的
问题混成同一个失败信号，排障时只能靠猜。

# 为什么自己解析而不是装第三方工具

`wasm-tools` / `twiggy` 都需要额外安装，且对「wasm-bindgen CLI 产物」
（custom section 已被剥离）的行为与预期不同。本工具只依赖标准库，
在任何 CI 镜像上都能直接跑。

# 用法

    python3 scripts/wasm-sections.py dist/assets/mkv-web_bg.wasm
    python3 scripts/wasm-sections.py dist/assets/mkv-web_bg.wasm --json

`--json` 输出机器可读结果（供 CI 门禁解析）：

    {
      "file_bytes": 706903,
      "sections": {"code": 557592, "data": 136543, "import": 7465, ...},
      "code_bytes": 557592,
      "data_bytes": 136543,
      "other_bytes": 12768
    }
"""

import argparse
import json
import os
import sys

# wasm 二进制格式的段 id（WebAssembly 规范 §5）
SECTION_NAMES = {
    0: "custom",
    1: "type",
    2: "import",
    3: "function",
    4: "table",
    5: "memory",
    6: "global",
    7: "export",
    8: "start",
    9: "element",
    10: "code",
    11: "data",
    12: "datacount",
}

# 计入「业务代码」的段：只有这些段会因写业务逻辑而变大。
CODE_SECTIONS = ("code", "type", "import", "function", "table", "export", "global")
# 计入「静态数据」的段：常量字符串、内嵌的索引产物等。
DATA_SECTIONS = ("data", "element", "start", "datacount")


def read_uleb(buf, i):
    """读一个 LEB128 无符号整数，返回 (值, 新位置)。"""
    result = 0
    shift = 0
    while True:
        if i >= len(buf):
            raise ValueError("LEB128 越界")
        byte = buf[i]
        i += 1
        result |= (byte & 0x7F) << shift
        if not (byte & 0x80):
            return result, i
        shift += 7


def parse_sections(buf):
    """解析 wasm 的段表，返回 [(段名, 字节数), ...]。"""
    if buf[:4] != b"\x00asm":
        raise ValueError("不是合法的 wasm 文件（magic 不匹配）")
    if len(buf) < 8:
        raise ValueError("wasm 文件过短")
    i = 8
    rows = []
    while i < len(buf):
        section_id = buf[i]
        i += 1
        size, i = read_uleb(buf, i)
        name = SECTION_NAMES.get(section_id, "id%d" % section_id)
        if section_id == 0:
            # custom 段：头部带段名
            name_len, j = read_uleb(buf, i)
            custom_name = buf[j:j + name_len].decode("utf-8", "replace")
            name = "custom:%s" % custom_name
        rows.append((name, size))
        i += size
    return rows


def analyze(path):
    buf = open(path, "rb").read()
    rows = parse_sections(buf)
    sections = {}
    for name, size in rows:
        # custom 段同名可能重复（多段同名），累加而不是覆盖
        sections[name] = sections.get(name, 0) + size
    code_bytes = sum(v for k, v in sections.items() if k in CODE_SECTIONS)
    data_bytes = sum(v for k, v in sections.items() if k in DATA_SECTIONS)
    other_bytes = len(buf) - code_bytes - data_bytes
    return {
        "file_bytes": len(buf),
        "sections": sections,
        "code_bytes": code_bytes,
        "data_bytes": data_bytes,
        "other_bytes": other_bytes,
    }


def human_report(result, path):
    total = result["file_bytes"]
    print("wasm 段构成：%s" % os.path.basename(path))
    print("  总大小: %d bytes" % total)
    print("  代码段合计: %d bytes (%.1f%%)" % (
        result["code_bytes"], 100.0 * result["code_bytes"] / total))
    print("  数据段合计: %d bytes (%.1f%%)" % (
        result["data_bytes"], 100.0 * result["data_bytes"] / total))
    print("  其他段合计: %d bytes (%.1f%%)" % (
        result["other_bytes"], 100.0 * result["other_bytes"] / total))
    print("  ── 逐段明细 ──")
    for name, size in sorted(result["sections"].items(), key=lambda kv: -kv[1]):
        print("    %-28s %8d  (%5.1f%%)" % (
            name, size, 100.0 * size / total))


def main(argv=None):
    parser = argparse.ArgumentParser(description="解析 wasm 段构成，用于分项体积归因")
    parser.add_argument("wasm", help="wasm 文件路径")
    parser.add_argument("--json", action="store_true", help="输出 JSON（供 CI 解析）")
    args = parser.parse_args(argv)

    if not os.path.isfile(args.wasm):
        print("::error::找不到 wasm：%s" % args.wasm, file=sys.stderr)
        return 2
    try:
        result = analyze(args.wasm)
    except ValueError as exc:
        print("::error::解析失败：%s" % exc, file=sys.stderr)
        return 2

    if args.json:
        print(json.dumps(result, ensure_ascii=False, sort_keys=True))
    else:
        human_report(result, args.wasm)
    return 0


if __name__ == "__main__":
    sys.exit(main())
