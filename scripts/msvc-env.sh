#!/usr/bin/env bash
# ============================================================
# Migrant Knowledge Vault — MSVC 构建环境注入脚本
#
# 用途：本机 Rust 为 windows-msvc 工具链，但 MSVC 的 cl.exe / link.exe
#       未在系统 PATH 中（VS2022 Community 已安装）。本脚本读取
#       VS 安装与 Windows SDK 版本，注入 PATH / INCLUDE / LIB，
#       供 rusqlite(bundled)、zip 等需要 C 编译的依赖使用。
#
# 用法：
#   source scripts/msvc-env.sh
#   cargo build
#
# 如已通过 Visual Studio Developer Command Prompt 打开终端，
# 则无需 source 本脚本（环境已就绪）。
# ============================================================

# --- 定位 Visual Studio ---
VS_ROOT="${VS_ROOT:-/c/Program Files/Microsoft Visual Studio/2022/Community}"
VC_VER="$(ls "$VS_ROOT/VC/Tools/MSVC" 2>/dev/null | sort -V | tail -1)"

if [ -z "$VC_VER" ]; then
  echo "[mkv] 未找到 MSVC 工具链，跳过环境注入。"
  echo "[mkv] 请安装 VS Build Tools（C++ 桌面开发）或打开 VS 开发者命令提示符。"
  return 1 2>/dev/null || exit 1
fi

VC_BIN="$VS_ROOT/VC/Tools/MSVC/$VC_VER/bin/Hostx64/x64"

# --- 定位 Windows SDK ---
SDK_ROOT="/c/Program Files (x86)/Windows Kits/10"
SDK_VER="$(ls "$SDK_ROOT/Include" 2>/dev/null | sort -V | tail -1)"

if [ -z "$SDK_VER" ]; then
  echo "[mkv] 未找到 Windows SDK，跳过环境注入。"
  return 1 2>/dev/null || exit 1
fi

SDK_INC="$SDK_ROOT/Include/$SDK_VER"
SDK_LIB="$SDK_ROOT/Lib/$SDK_VER"

# --- 注入环境 ---
export PATH="$VC_BIN:$SDK_ROOT/bin/$SDK_VER/x64:$PATH"

export INCLUDE="$VC_BIN/../../include;$SDK_INC/ucrt;$SDK_INC/um;$SDK_INC/shared;$SDK_INC/winrt"
export LIB="$VC_BIN/../../lib/x64;$SDK_LIB/ucrt/x64;$SDK_LIB/um/x64"

echo "[mkv] MSVC 环境已注入"
echo "[mkv]   VC   : $VC_VER"
echo "[mkv]   SDK  : $SDK_VER"
echo "[mkv]   cl   : $(command -v cl.exe || echo '未找到')"
