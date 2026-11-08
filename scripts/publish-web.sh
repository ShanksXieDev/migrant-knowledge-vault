#!/usr/bin/env bash
# ============================================================
# Migrant Knowledge Vault —— Web 端静态发布（方案 A）
#
# 产出一份**纯静态、零后端、可离线**的站点到 dist/：
#   1. 从知识库导出预构建只读索引（native，jieba + 领域词典）
#   2. dx build --release 把索引嵌入 wasm 二进制
#   3. 只复制 index.html 实际引用的资源，重写绝对路径为相对路径
#      （支持 GitHub Pages 子路径托管；也避免 dx public 里残留的旧 hash 资源）
#   4. 附带一份独立 mkv-index.json，供远端热替换（可选）
#
# 用法：
#   scripts/publish-web.sh [vault_dir] [dict.txt]
#   DIST=out/ scripts/publish-web.sh
#   MKV_WEB_INDEX_URL=https://example.com/mkv-index.json scripts/publish-web.sh
#
# 部署：把 dist/ 整个目录上传到任意静态托管即可
#   （GitHub Pages / 对象存储 / nginx / python -m http.server）。
# ============================================================
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

VAULT="${1:-examples/sample-vault}"
DICT="${2:-${DICT:-examples/domain-dict.txt}}"
DIST="${DIST:-dist}"
INDEX_OUT="packages/web/assets/mkv-index.json"
PUBLIC_DIR="target/dx/mkv-web/release/web/public"
# Windows 常只有 python、Linux CI 常只有 python3
PY="$(command -v python3 || command -v python)"

echo "[mkv] 知识库：$VAULT"
echo "[mkv] 输出目录：$DIST"

# --- 1. 导出预构建索引产物（会顺带刷新库内索引）---
echo "[mkv] 1/4 导出预构建索引…"
if [ -f "$DICT" ]; then
    cargo run --quiet -p mkv-cli -- export-web "$VAULT" "$INDEX_OUT" "$DICT"
else
    cargo run --quiet -p mkv-cli -- export-web "$VAULT" "$INDEX_OUT"
fi

# --- 2. release 构建（索引被 include_str! 编进 wasm）---
echo "[mkv] 2/4 构建 release wasm（首次会自动安装 wasm-opt，约 1–2 分钟）…"
if [ -n "${MKV_WEB_INDEX_URL:-}" ]; then
    echo "[mkv]      远端索引地址：$MKV_WEB_INDEX_URL"
    MKV_WEB_INDEX_URL="$MKV_WEB_INDEX_URL" dx build --release --web --package mkv-web
else
    dx build --release --web --package mkv-web
fi

if [ ! -f "$PUBLIC_DIR/index.html" ]; then
    echo "[mkv] 错误：未找到 dx release 产物 $PUBLIC_DIR/index.html" >&2
    exit 1
fi

# --- 3/4. 精确组装静态站点 ---
echo "[mkv] 3/4 组装静态站点（路径重写 + 剔除残留资源）…"
echo "[mkv] 4/4 附带可替换的索引产物…"

"$PY" - "$PUBLIC_DIR" "$DIST" "$INDEX_OUT" "$VAULT" "${MKV_WEB_INDEX_URL:-}" <<'PYEOF'
import datetime
import io
import json
import os
import re
import shutil
import sys

public, dist, index_out, vault, remote = sys.argv[1:6]

# 清空 dist（只动本项目的产物目录）
if os.path.isdir(dist):
    for name in os.listdir(dist):
        p = os.path.join(dist, name)
        shutil.rmtree(p) if os.path.isdir(p) else os.remove(p)
os.makedirs(os.path.join(dist, 'assets'), exist_ok=True)

# dx 生成的是 "/./assets/..."，部署到子路径会 404 → 改为 "./assets/..."
def relativize(text):
    return text.replace('/./assets/', './assets/')

html = io.open(os.path.join(public, 'index.html'), encoding='utf-8').read()
html = relativize(html)
# 内联空图标：避免静态托管下浏览器请求 /favicon.ico 产生 404 console error
html = html.replace('<head>', '<head>\n    <link rel="icon" href="data:,">', 1)
io.open(os.path.join(dist, 'index.html'), 'w', encoding='utf-8', newline='').write(html)

# 只复制 index.html 引用的 JS，再只复制该 JS 引用的 wasm
copied = {}
js_names = set(re.findall(r'\./assets/([^"\']+\.js)', html))
for js_name in js_names:
    src = os.path.join(public, 'assets', js_name)
    if not os.path.isfile(src):
        raise SystemExit(f'[mkv] 缺少 JS 资源: {src}')
    text = io.open(src, encoding='utf-8').read()
    text = relativize(text)
    dst = os.path.join(dist, 'assets', js_name)
    io.open(dst, 'w', encoding='utf-8', newline='').write(text)
    copied[f'assets/{js_name}'] = os.path.getsize(dst)

    for wasm_name in set(re.findall(r'\./assets/([^"\']+\.wasm)', text)):
        wsrc = os.path.join(public, 'assets', wasm_name)
        if not os.path.isfile(wsrc):
            raise SystemExit(f'[mkv] 缺少 wasm 资源: {wsrc}')
        wdst = os.path.join(dist, 'assets', wasm_name)
        shutil.copy2(wsrc, wdst)
        copied[f'assets/{wasm_name}'] = os.path.getsize(wdst)

if not any(k.endswith('.wasm') for k in copied):
    raise SystemExit('[mkv] 未找到 wasm 资源，站点不可用')

# GitHub Pages 默认走 Jekyll，可能忽略下划线开头的资源
io.open(os.path.join(dist, '.nojekyll'), 'w').close()
# 独立索引产物：替换它即可更新索引（需构建时配置 MKV_WEB_INDEX_URL）
shutil.copy2(index_out, os.path.join(dist, 'mkv-index.json'))

files = {
    'index.html': os.path.getsize(os.path.join(dist, 'index.html')),
    '.nojekyll': 0,
    'mkv-index.json': os.path.getsize(os.path.join(dist, 'mkv-index.json')),
    **copied,
}
info = {
    'generated_at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'vault': vault,
    'deployment': 'static (plan A)',
    'remote_index_url': remote or None,
    'files': files,
}
io.open(os.path.join(dist, 'deploy-info.json'), 'w', encoding='utf-8').write(
    json.dumps(info, ensure_ascii=False, indent=2)
)

print('      产物清单：')
for k in sorted(files):
    print(f'        {k}  {files[k]} bytes')
PYEOF

echo "[mkv] 完成：$(cd "$DIST" && pwd)"
echo "[mkv] 本地预览：$PY -m http.server 8080 --directory $DIST"
if [ -z "${MKV_WEB_INDEX_URL:-}" ]; then
    echo "[mkv] 提示：未设置 MKV_WEB_INDEX_URL，索引仅内嵌于 wasm；"
    echo "[mkv]       如需「替换 JSON 即更新索引、免重编 wasm」，可用该环境变量重新构建。"
fi
