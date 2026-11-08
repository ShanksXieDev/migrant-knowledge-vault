#!/usr/bin/env bash
# ============================================================
# Migrant Knowledge Vault —— Web 端（wasm32）构建脚本
#
# 用途：一键完成「生成预构建索引 → 嵌入 wasm 二进制」的全流程。
#
# 背景（技术方案设计 §4 / 决策②）：
#   Web 端（wasm32）无法运行 Tantivy（tantivy → zstd → C 编译器），
#   也不适合在浏览器里实时建索引，因此采用**预构建只读索引**：
#   构建期由 native 端生成索引产物，随站打包，运行时只读加载。
#
# 用法：
#   scripts/build-web.sh [vault_dir] [out_json]
#
# 默认：
#   vault_dir = examples/sample-vault
#   out_json  = packages/web/assets/mkv-index.json
#
# 产物会落到约定路径 packages/web/assets/mkv-index.json，
# 由 packages/web/build.rs 在编译期复制进 OUT_DIR 并被 include_str! 嵌入。
# ============================================================
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

VAULT="${1:-examples/sample-vault}"
OUT="${2:-packages/web/assets/mkv-index.json}"
DICT="${DICT:-examples/domain-dict.txt}"

# --- 1. 生成预构建索引产物（native 端）---
echo "[mkv] 生成预构建索引：$VAULT → $OUT"
if [ -f "$DICT" ]; then
    cargo run --quiet -p mkv-cli -- export-web "$VAULT" "$OUT" "$DICT"
else
    cargo run --quiet -p mkv-cli -- export-web "$VAULT" "$OUT"
fi

# --- 2. 构建 wasm（产物被嵌入二进制）---
echo "[mkv] 构建 wasm32 → target/wasm32-unknown-unknown/debug/mkv-web.wasm"
cargo build -p mkv-web --target wasm32-unknown-unknown

echo "[mkv] 完成。"
echo "     如需远端热替换索引，构建时追加："
echo "       MKV_WEB_INDEX_URL=https://example.com/mkv-index.json scripts/build-web.sh"
echo "     如需本地预览：dx serve --platform web   （需安装 Dioxus CLI）"
