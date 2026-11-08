#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""上游编辑器插件同步工具。

三个 Craft 项目（storytold/{gridcraft,deckcraft,wordcraft}）是独立 Rust workspace。
本脚本负责「检测到更新之后」的源码同步，不改动插件契约代码：

    python scripts/sync_editors.py --check
        探测三个上游默认分支 HEAD，与 editors.lock 的 pin 对比，
        输出 docs/editor-updates/report.json 并在终端打印摘要。

    python scripts/sync_editors.py --apply [--root <知识库目录>]
        按 pin rev（或 --root 指定知识库里用户已确认的 pending rev）
        把上游源码同步到 .plugins/sources/<id>，写 .plugins/upstream-manifest.json。

重要边界：插件是编译期 crate，同步源码后必须重新构建并重启应用才会生效；
脚本不会假装能在运行时替换已链接进二进制的代码。
"""

import argparse
import datetime
import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
LOCK = ROOT / "editors.lock"
SOURCES = ROOT / ".plugins" / "sources"
MANIFEST = ROOT / ".plugins" / "upstream-manifest.json"
REPORT = ROOT / "docs" / "editor-updates" / "report.json"


def now_utc() -> str:
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def load_json(path: pathlib.Path):
    return json.loads(path.read_text(encoding="utf-8"))


def git(*args, cwd=None, timeout=60):
    return subprocess.run(
        ["git", *args],
        cwd=str(cwd) if cwd else None,
        capture_output=True,
        text=True,
        timeout=timeout,
    )


def probe(repo: str, branch: str):
    """返回上游分支 HEAD；两个通道都失败返回 None（离线时静默降级）。

    实测某些代理会对 git 的 CONNECT 隧道返回 502，但普通 HTTPS GET 正常，
    因此 git ls-remote 失败后回退到 GitHub commits API（与宿主 core 一致）。
    """
    try:
        r = git("-c", "http.lowSpeedLimit=1000", "-c", "http.lowSpeedTime=15",
                "ls-remote", f"https://github.com/{repo}.git", f"refs/heads/{branch}")
        if r.returncode == 0:
            parts = r.stdout.split()
            if parts and len(parts[0]) >= 7:
                return parts[0]
    except (OSError, subprocess.TimeoutExpired):
        pass
    try:
        r = subprocess.run(
            ["curl", "-sS", "--max-time", "15",
             "-H", "Accept: application/vnd.github+json",
             f"https://api.github.com/repos/{repo}/commits/{branch}"],
            capture_output=True, text=True, timeout=25,
        )
        if r.returncode == 0:
            sha = json.loads(r.stdout).get("sha")
            if sha and len(sha) >= 7:
                return sha
    except (OSError, subprocess.TimeoutExpired, json.JSONDecodeError):
        pass
    return None


def user_confirmed_revisions(root: str):
    """读取知识库 .kbindex/editors.json 里用户已确认的 pending 目标 rev。"""
    if not root:
        return {}
    state = pathlib.Path(root) / ".kbindex" / "editors.json"
    if not state.exists():
        return {}
    try:
        data = load_json(state)
    except (OSError, json.JSONDecodeError):
        return {}
    return {p["plugin_id"]: p["to_rev"] for p in data.get("pending", [])
            if p.get("plugin_id") and p.get("to_rev")}


def cmd_check(_args):
    lock = load_json(LOCK)
    rows = []
    for p in lock["plugins"]:
        head = probe(p["repo"], p["branch"])
        if head is None:
            status = "probe-failed"
        elif head == p["pinned_rev"]:
            status = "up-to-date"
        else:
            status = "update-available"
        rows.append({
            "id": p["id"],
            "repo": p["repo"],
            "branch": p["branch"],
            "pinned_rev": p["pinned_rev"],
            "remote_head": head,
            "status": status,
        })
        print(f"  {p['id']:<10} {status:<17} pin={p['pinned_rev'][:12]} head={(head or '-')[:12]}")
    REPORT.parent.mkdir(parents=True, exist_ok=True)
    REPORT.write_text(
        json.dumps({
            "checked_at": now_utc(),
            "contract_version": lock["contract_version"],
            "plugins": rows,
        }, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    updates = [r for r in rows if r["status"] == "update-available"]
    print(f"[mkv-editors] {len(rows)} 个插件，{len(updates)} 个有更新；报告 {REPORT}")
    return 0


def cmd_apply(args):
    lock = load_json(LOCK)
    confirmed = user_confirmed_revisions(args.root)
    SOURCES.mkdir(parents=True, exist_ok=True)
    entries = []
    for p in lock["plugins"]:
        rev = confirmed.get(p["id"], p["pinned_rev"])
        url = f"https://github.com/{p['repo']}.git"
        dest = SOURCES / p["id"]
        if not (dest / ".git").exists():
            print(f"[mkv-editors] clone {p['repo']} -> {dest}")
            r = git("clone", "--filter=blob:none", "--no-checkout", url, str(dest), timeout=300)
            if r.returncode != 0:
                print(f"[mkv-editors] clone 失败: {r.stderr.strip()}", file=sys.stderr)
                return 1
        r = git("fetch", "--depth", "1", "origin", rev, cwd=dest, timeout=300)
        if r.returncode != 0:
            print(f"[mkv-editors] fetch {rev[:12]} 失败: {r.stderr.strip()}", file=sys.stderr)
            return 1
        r = git("checkout", "--detach", rev, cwd=dest, timeout=300)
        if r.returncode != 0:
            print(f"[mkv-editors] checkout 失败: {r.stderr.strip()}", file=sys.stderr)
            return 1
        actual = git("rev-parse", "HEAD", cwd=dest).stdout.strip()
        entries.append({
            "id": p["id"],
            "repo": p["repo"],
            "rev": actual,
            "from_user_confirmation": p["id"] in confirmed,
            "source_dir": str(dest.relative_to(ROOT)).replace("\\", "/"),
        })
        print(f"  {p['id']:<10} -> {actual[:12]} ({'用户确认' if p['id'] in confirmed else '锁文件 pin'})")
    MANIFEST.write_text(
        json.dumps({
            "generated_at": now_utc(),
            "contract_version": lock["contract_version"],
            "plugins": entries,
            "note": "插件为编译期 crate；同步源码后需 cargo build --workspace 并重启应用生效",
        }, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    print(f"[mkv-editors] 清单 {MANIFEST}")
    print("[mkv-editors] 已完成源码同步；下一步 cargo build --workspace 重建插件并重启应用。")
    return 0


def main():
    ap = argparse.ArgumentParser(description="同步三个上游 Craft 编辑器仓库")
    ap.add_argument("--check", dest="mode", action="store_const", const="check")
    ap.add_argument("--apply", dest="mode", action="store_const", const="apply")
    ap.add_argument("--root", default="", help="知识库目录（读取用户已确认的 pending rev）")
    ap.set_defaults(mode="check")
    args = ap.parse_args()
    if not LOCK.exists():
        print(f"[mkv-editors] 缺少 {LOCK}", file=sys.stderr)
        return 1
    return cmd_check(args) if args.mode == "check" else cmd_apply(args)


if __name__ == "__main__":
    sys.exit(main())
