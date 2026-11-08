#!/usr/bin/env bash
# 上游编辑器插件同步（薄包装，实际逻辑在 sync_editors.py）。
#
#   bash scripts/sync-editors.sh --check
#   bash scripts/sync-editors.sh --apply [--root <知识库目录>]
#
# --check 只做只读探测；--apply 按锁文件 / 用户确认的 rev 同步源码到 .plugins/sources/。
# 插件是编译期 crate，同步后仍需 cargo build --workspace 并重启应用才生效。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PYTHON="${MKV_PYTHON:-}"
if [ -z "$PYTHON" ]; then
  if command -v python >/dev/null 2>&1; then
    PYTHON="$(command -v python)"
  else
    PYTHON="python3"
  fi
fi

exec "$PYTHON" "$ROOT/scripts/sync_editors.py" "$@"
