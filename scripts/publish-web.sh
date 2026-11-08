#!/usr/bin/env bash
# ============================================================
# Migrant Knowledge Vault —— Web 端静态发布（方案 A）
#
# 产出一份**纯静态、零后端、可离线**的站点到 dist/：
#   1. 从知识库导出预构建只读索引（native，jieba + 领域词典）
#   2. dx build --release 把索引嵌入 wasm 二进制
#   3. 只复制 index.html 实际引用的资源，重写绝对路径为相对路径
#      （支持 GitHub Pages 子路径托管；也避免 dx public 里残留的旧 hash 资源）；
#      同时用 wasm-opt -Oz --strip-debug 剥离 dx 默认生成的 DWARF 调试段并压缩 wasm
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

# --- 定位 wasm-opt ---------------------------------------------------------
# dx release 构建时会自动下载 binaryen 到 ~/.dx/tools/binaryen-*/bin/。
# 优先用 dx 自带的那份（版本与 wasm-bindgen 产物匹配），PATH 仅作兜底：
# CI runner 上若预装了较老的 binaryen，老版本可能解析不了新版 wasm 特性。
# 找不到时只警告、不阻断发布；workflow 的体积门禁会让 CI 明确失败而不是静默发布。
WASM_OPT=""
WASM_OPT_SOURCE=""
for candidate in "$HOME"/.dx/tools/binaryen-*/bin/wasm-opt \
                 "$HOME"/.dx/tools/binaryen-*/bin/wasm-opt.exe \
                 "$HOME"/.dx/tools/binaryen-*/wasm-opt \
                 "$HOME"/.dx/tools/binaryen-*/wasm-opt.exe; do
    if [ -x "$candidate" ]; then WASM_OPT="$candidate"; WASM_OPT_SOURCE="dx-cache"; break; fi
done
if [ -z "$WASM_OPT" ] && command -v wasm-opt >/dev/null 2>&1; then
    WASM_OPT="$(command -v wasm-opt)"
    WASM_OPT_SOURCE="path"
fi
if [ -n "$WASM_OPT" ]; then
    echo "[mkv]      wasm-opt：$WASM_OPT（来源：$WASM_OPT_SOURCE）"
    WASM_OPT_VERSION="$("$WASM_OPT" --version 2>/dev/null | head -n1 || echo '未知')"
    echo "[mkv]      wasm-opt 版本：$WASM_OPT_VERSION"
else
    # 这里不静默：体积门禁会因此失败，必须让原因在日志里一眼可见
    # （run #15 就因为 dx 未成功下载 binaryen 导致 wasm 未优化、门禁失败，
    #   而当时的日志里没有任何 wasm-opt 相关线索，排查成本很高）。
    echo "[mkv]      警告：未找到 wasm-opt，本次跳过体积优化" >&2
    echo "[mkv]      提示：dx 构建时应自动下载 binaryen 到 ~/.dx/tools/；" >&2
    echo "[mkv]            若该步骤失败，请检查 GitHub Release 下载是否可达。" >&2
    WASM_OPT_VERSION="none"
fi

# --- 3/4. 精确组装静态站点 + wasm 体积优化 ---
echo "[mkv] 3/4 组装静态站点（路径重写 + 剔除残留资源）…"
echo "[mkv] 4/4 附带可替换的索引产物…"

MKV_WASM_OPT="$WASM_OPT" MKV_WASM_OPT_VERSION="$WASM_OPT_VERSION" \
    "$PY" - "$PUBLIC_DIR" "$DIST" "$INDEX_OUT" "$VAULT" "${MKV_WEB_INDEX_URL:-}" <<'PYEOF'
import datetime
import io
import json
import os
import re
import shutil
import subprocess
import sys

public, dist, index_out, vault, remote = sys.argv[1:6]

# dx 默认 --debug-symbols=true，release wasm 里带大量 DWARF 调试段（约占总大小一半）。
# wasm-opt 作用于 wasm-bindgen 输出之后，剥离调试段不会碰 rustc 的
# __wasm_bindgen_unstable 等自定义段，因此 Cargo.toml 仍保持 strip=false。
# 优先用最小参数集（-Oz --strip-debug，binaryen 各版本均支持）；
# 更完整的参数集作为后续尝试，失败则保留原始 wasm，绝不因优化让站点不可用。
def normalize_tool_path(p):
    # Git Bash 传进来的是 POSIX 路径（/c/Users/...），Windows 原生 Python
    # 会把它当成 C:\c\Users\... 而找不到文件。转成 C:/Users/... 即可；
    # Linux 的 /home/... 不匹配（第二级不是单字母），原样返回。
    m = re.match(r'^/([a-zA-Z])/(.*)$', p)
    return m.group(1).upper() + ':/' + m.group(2) if m else p

wasm_opt = normalize_tool_path(os.environ.get('MKV_WASM_OPT', '').strip())
# Git Bash 认为无扩展名的 wasm-opt 可执行，但 Windows 上真实文件是 wasm-opt.exe
if wasm_opt and not os.path.isfile(wasm_opt) and os.path.isfile(wasm_opt + '.exe'):
    wasm_opt += '.exe'
wasm_optimization = {}
# 优化失败原因：写进 deploy-info.json，让 CI 门禁能读到结构化原因，
# 而不是只能从日志里猜（run #15 的排查就是这么卡住的）。
wasm_opt_issue = None

def optimize_wasm(tool, dst):
    global wasm_opt_issue
    if not tool or not os.path.isfile(tool):
        print('      [warn] 未找到 wasm-opt，跳过体积优化')
        wasm_opt_issue = 'wasm-opt 不可用：找不到可执行文件'
        return None
    before = os.path.getsize(dst)
    tmp = dst + '.opt'
    # 按「保守 → 激进」排列，逐组尝试，任一组成功即采用。
    # 第一组只含各版本 binaryen 都支持的基础参数；
    # 第二、三组含更多剥离项，旧版本 binaryen 可能不认识，会自动跳到下一组。
    flag_sets = [
        ['-Oz', '--strip-debug'],
        ['-Oz', '--strip-debug', '--strip-dwarf'],
        ['-Oz', '--strip-debug', '--strip-dwarf', '--strip-producers'],
    ]
    for flags in flag_sets:
        r = subprocess.run([tool, *flags, dst, '-o', tmp],
                           stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        if r.returncode == 0 and os.path.isfile(tmp):
            after = os.path.getsize(tmp)
            if after < before:
                os.replace(tmp, dst)
                return {'before': before, 'after': after,
                        'flags': ' '.join(flags)}
            reason = '产物未变小'
        else:
            reason = (r.stderr or b'').decode('utf-8', 'replace').strip()[:300] \
                     or '无 stderr 输出'
        # CI 上 binaryen 版本不兼容时，把真实原因打进日志，便于直接排障
        print('      [warn] wasm-opt %s 未采用（exit %d）：%s'
              % (' '.join(flags), r.returncode, reason))
        wasm_opt_issue = 'wasm-opt %s 失败（exit %d）：%s' % (
            ' '.join(flags), r.returncode, reason)
        if os.path.isfile(tmp):
            os.remove(tmp)
    print('      [warn] wasm-opt 未产出更小的产物，保留原始 wasm')
    return None

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
        record = optimize_wasm(wasm_opt, wdst)
        if record:
            wasm_optimization[f'assets/{wasm_name}'] = record
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
    'wasm_optimization': wasm_optimization or None,
    'wasm_opt_issue': wasm_opt_issue,
    'wasm_opt_version': os.environ.get('MKV_WASM_OPT_VERSION') or None,
    'files': files,
}
io.open(os.path.join(dist, 'deploy-info.json'), 'w', encoding='utf-8').write(
    json.dumps(info, ensure_ascii=False, indent=2)
)

print('      产物清单：')
for k in sorted(files):
    print(f'        {k}  {files[k]} bytes')
for k, rec in wasm_optimization.items():
    pct = 100.0 * (1 - rec['after'] / rec['before'])
    print(f'      wasm 优化 {k}: {rec["before"]} → {rec["after"]} bytes (-{pct:.1f}%)')
PYEOF

echo "[mkv] 完成：$(cd "$DIST" && pwd)"
echo "[mkv] 本地预览：$PY -m http.server 8080 --directory $DIST"
if [ -z "${MKV_WEB_INDEX_URL:-}" ]; then
    echo "[mkv] 提示：未设置 MKV_WEB_INDEX_URL，索引仅内嵌于 wasm；"
    echo "[mkv]       如需「替换 JSON 即更新索引、免重编 wasm」，可用该环境变量重新构建。"
fi
