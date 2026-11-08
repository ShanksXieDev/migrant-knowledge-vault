"""正则引擎差分对拍：比对 Rust 引擎与 Python `re` 的输出。

用法：
    python packages/core/examples/regex_fuzz_ref.py > .fuzz_tmp/ref.tsv
    cargo run --release -p mkv-core --no-default-features --features web \
        --example regex_fuzz_run < .fuzz_tmp/ref.tsv > .fuzz_tmp/got.tsv
    python packages/core/examples/regex_fuzz_cmp.py .fuzz_tmp/ref.tsv .fuzz_tmp/got.tsv

分类约定：
- 含 `$` 的模式：Rust 侧采用**严格末尾**语义（与 Rust `regex` crate 一致），
  Python `re` 允许在末尾换行前匹配 → 属**有意偏差**，不计入缺陷。
- 其余差异计为**真实差异**。
"""

import binascii
import sys


def unhex(s: str) -> str:
    if not s:
        return ""
    return binascii.unhexlify(s.encode("ascii")).decode("utf-8")


def load(path: str):
    out = []
    with open(path, "r", encoding="utf-8") as fh:
        for line in fh:
            line = line.rstrip("\n")
            if not line:
                continue
            parts = line.split("\t")
            if len(parts) < 3:
                continue
            out.append((unhex(parts[0]), unhex(parts[1]), parts[2], parts[3] if len(parts) > 3 else ""))
    return out


def main() -> int:
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    ref = load(sys.argv[1])
    got = load(sys.argv[2])
    if len(ref) != len(got):
        print(f"行数不一致：ref={len(ref)} got={len(got)}")

    dollar = []
    real = []
    for i in range(min(len(ref), len(got))):
        rp, rt, rhit, rall = ref[i]
        gp, gt, ghit, gall = got[i]
        if (rp, rt) != (gp, gt):
            print(f"[{i}] 用例错位：ref={rp!r}/{rt!r} got={gp!r}/{gt!r}")
            continue
        if rhit == ghit and rall == gall:
            continue
        entry = (rp, rt, rhit, rall, ghit, gall)
        (dollar if "$" in rp else real).append(entry)

    print("=" * 72)
    print(f"总用例        : {min(len(ref), len(got))}")
    print(f"含 $ 有意偏差 : {len(dollar)}")
    print(f"真实差异      : {len(real)}")
    print("=" * 72)
    for title, rows in (("含 $（有意偏差，不计缺陷）", dollar), ("真实差异（需修）", real)):
        print(f"\n--- {title} ---")
        for rp, rt, rhit, rall, ghit, gall in rows:
            print(f"  pat={rp!r} text={rt!r}")
            print(f"    py  hit={rhit} all={rall}")
            print(f"    rs  hit={ghit} all={gall}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
