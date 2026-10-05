#!/usr/bin/env bash
set -e

# 用法：./build.sh [release|debug]，默认 release
PROFILE="${1:-release}"
TARGET="x86_64-pc-windows-gnu"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# 1. 构建流水号：日期.当日序号 (如 20260918.01)，记录到 build-id.txt
TODAY=$(date +%Y%m%d)
SEQ=1
if [ -f "build-id.txt" ]; then
    LAST=$(tr -d '\r\n' < build-id.txt)
    if [[ "$LAST" =~ ^$TODAY\.([0-9]+)$ ]]; then
        SEQ=$(( 10#${BASH_REMATCH[1]} + 1 ))
    fi
fi
BUILD_ID=$(printf "%s.%02d" "$TODAY" "$SEQ")
printf "%s" "$BUILD_ID" > build-id.txt

GIT_HASH=$(git rev-parse --short=7 HEAD 2>/dev/null || echo "nogit")
mkdir -p Temp
printf "%s\t%s\t%s\t%s\n" "$BUILD_ID" "$GIT_HASH" "$(date '+%Y-%m-%d %H:%M:%S')" "$PROFILE" >> Temp/build-history.log
echo "构建号：build$BUILD_ID"

# 2. 设置构建参数与产物目录
CARGO_FLAGS=("--target" "$TARGET")
if [ "$PROFILE" = "release" ]; then
    CARGO_FLAGS+=("--release")
    SRC="target/$TARGET/release"
    CONFIG_UI_SRC="config-ui/target/$TARGET/release"
else
    SRC="target/$TARGET/debug"
    CONFIG_UI_SRC="config-ui/target/$TARGET/debug"
fi

# 辅助函数：修复 OpenSSL 命名以兼容 MinGW ld
fix_openssl_symlinks() {
    find "$ROOT_DIR" -type d -path "*/openssl-build/install/lib" 2>/dev/null | while read -r lib_dir; do
        [ -f "$lib_dir/libcrypto.a" ] && [ ! -e "$lib_dir/liblibcrypto.a" ] && ln -sf libcrypto.a "$lib_dir/liblibcrypto.a"
        [ -f "$lib_dir/libssl.a" ] && [ ! -e "$lib_dir/liblibssl.a" ] && ln -sf libssl.a "$lib_dir/liblibssl.a"
    done || true
}

# 辅助函数：将 assets/windows/shaders_bytes.rs 补充到现有的 gpui_windows 构建缓存
sync_shaders_to_out_dirs() {
    local shader_src="$ROOT_DIR/assets/windows/shaders_bytes.rs"
    if [ -f "$shader_src" ]; then
        find "$ROOT_DIR" -type d -path "*/build/gpui_windows-*/out" 2>/dev/null | while read -r out_dir; do
            if [ ! -f "$out_dir/shaders_bytes.rs" ]; then
                cp -f "$shader_src" "$out_dir/"
            fi
        done || true
    fi
}

# 3. 准备 gpui 交叉编译补丁（着色器回退）
GPUI_BUILD_RS="$ROOT_DIR/Ref-src/zed/crates/gpui_windows/build.rs"
if [ -f "$GPUI_BUILD_RS" ]; then
    if ! grep -q "shaders_bytes.rs fallback" "$GPUI_BUILD_RS"; then
        python3 - << 'EOF'
import os
build_rs = "Ref-src/zed/crates/gpui_windows/build.rs"
if os.path.exists(build_rs):
    with open(build_rs, "r", encoding="utf-8") as f:
        content = f.read()
    patch = '''
    // shaders_bytes.rs fallback for cross-compilation
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let target_shader = std::path::Path::new(&out_dir).join("shaders_bytes.rs");
    if !target_shader.exists() {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let fallback_shader = manifest_dir.join("../../../../assets/windows/shaders_bytes.rs");
        if fallback_shader.exists() {
            let _ = std::fs::copy(&fallback_shader, &target_shader);
        }
    }
'''
    if "fn main() {" in content:
        content = content.replace("fn main() {", "fn main() {" + patch, 1)
        with open(build_rs, "w", encoding="utf-8") as f:
            f.write(content)
        print("==> 已为 gpui_windows 注入跨平台着色器回退补丁")
EOF
    fi
fi

# 4. 编译主工程 (带 OpenSSL 失败自动修复重试)
echo "==> 正在编译主工程 (orca-term)..."
fix_openssl_symlinks
if ! cargo build "${CARGO_FLAGS[@]}"; then
    echo "==> 检测到链接可能缺失 OpenSSL 别名，修复软链接后重试..."
    fix_openssl_symlinks
    cargo build "${CARGO_FLAGS[@]}"
fi

# 5. 编译独立的 config-ui 工程
echo "==> 正在编译 config-ui..."
(
    cd config-ui
    sync_shaders_to_out_dirs
    fix_openssl_symlinks
    if ! cargo build "${CARGO_FLAGS[@]}"; then
        echo "==> config-ui 构建中断，同步着色器并重试..."
        sync_shaders_to_out_dirs
        fix_openssl_symlinks
        cargo build "${CARGO_FLAGS[@]}"
    fi
)

# 6. 打包提取文件到 dist/ 目录
echo "==> 正在复制并整理到 dist/ 目录..."
mkdir -p dist

cp -f "$SRC/orca-term-gui.exe" dist/
cp -f "$SRC/orca-term.exe" dist/
cp -f "$CONFIG_UI_SRC/orca-term-config-ui.exe" dist/

# 复制 ConPTY 运行时依赖
cp -f "assets/windows/conhost/conpty.dll" dist/
cp -f "assets/windows/conhost/OpenConsole.exe" dist/

# 7. 剥离调试符号（缩减体积）
if command -v x86_64-w64-mingw32-strip >/dev/null 2>&1 && [ "$PROFILE" = "release" ]; then
    echo "==> 正在使用 strip 缩减可执行文件体积..."
    x86_64-w64-mingw32-strip dist/*.exe
fi

echo "=========================================="
echo "构建完成！dist/ 内容清单："
ls -lh dist/
