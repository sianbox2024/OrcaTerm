#!/usr/bin/env bash
# OrcaTerm 打包辅助脚本（本机临时用）
# 等价于 build.sh 的第 6-7 步：整理 dist/ 并 strip。
# 与 build.sh 的差异：config-ui 依赖 gitignore 的 ../Ref-src/zed-1.16.1，
# 本机没有该目录，因此本脚本跳过 config-ui，只打包主工程产物。
set -e

TARGET="x86_64-pc-windows-gnu"
SRC="target/$TARGET/release"

mkdir -p dist
cp -f "$SRC/orca-term-gui.exe" dist/
cp -f "$SRC/orca-term.exe" dist/
cp -f "assets/windows/conhost/conpty.dll" dist/
cp -f "assets/windows/conhost/OpenConsole.exe" dist/

if command -v x86_64-w64-mingw32-strip >/dev/null 2>&1; then
    echo "==> strip 缩减体积..."
    x86_64-w64-mingw32-strip dist/*.exe
fi

echo "=========================================="
echo "构建完成！dist/ 内容清单："
ls -lh dist/
