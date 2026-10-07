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

# 辅助函数：下载 starship 静态 musl 二进制到 dist/starship/，
# 供 SSH 提示符注入在离线环境使用（连接时优先从程序同目录取）；
# 同时下载 Windows 版 starship.exe 到 dist/ 根目录，供本机 PowerShell
# 提示符使用（如用户配置里引用的「程序目录\starship.exe」）。
# 版本号以 wezterm-ssh/src/orca_prompt.rs 的 STARSHIP_VERSION 为单一事实源。
fetch_starship() {
    local ver
    ver=$(grep -o 'STARSHIP_VERSION: &str = "[0-9.]*"' "$ROOT_DIR/wezterm-ssh/src/orca_prompt.rs" 2>/dev/null | head -1 | cut -d'"' -f2)
    if [ -z "$ver" ]; then
        echo "==> 警告：无法从 orca_prompt.rs 解析 STARSHIP_VERSION，跳过 dist/starship 下载"
        return 0
    fi
    # 版本变化时清掉旧版本产物，避免目录里残留过期二进制
    if [ -f "dist/starship/.version" ] && [ "$(cat dist/starship/.version)" != "$ver" ]; then
        rm -rf dist/starship
    fi
    mkdir -p dist/starship
    printf %s "$ver" > dist/starship/.version

    # starship 相关产物统一放 dist/starship/（本机 PowerShell 与远端注入共用）：
    # - starship.toml：gruvbox powerline 预设（Issue/starship.toml），本机经
    #   STARSHIP_CONFIG 引用，SSH 注入时也上传到远端；
    # - starship.exe：Windows 版，本机 PowerShell 启动菜单经
    #   & '…\starship\starship.exe' init powershell 引用（Issue/1.png）。
    if [ -f "$ROOT_DIR/assets/starship.toml" ]; then
        cp -f "$ROOT_DIR/assets/starship.toml" "dist/starship/starship.toml"
    else
        echo "==> 警告：缺少 assets/starship.toml，starship 提示符将使用出厂默认样式"
    fi

    local base="https://github.com/starship/starship/releases/download/v$ver"
    local target
    for target in x86_64-unknown-linux-musl aarch64-unknown-linux-musl; do
        local out="dist/starship/starship-$target"
        if [ -s "$out" ]; then
            continue
        fi
        echo "==> 下载 starship v$ver ($target)..."
        local tmp
        tmp=$(mktemp -d)
        if curl -fsSL --retry 3 --retry-delay 2 --retry-all-errors "$base/starship-$target.tar.gz" -o "$tmp/s.tar.gz" \
            && curl -fsSL --retry 3 --retry-delay 2 --retry-all-errors "$base/starship-$target.tar.gz.sha256" -o "$tmp/s.sha256" \
            && echo "$(awk '{print $1}' "$tmp/s.sha256")  $tmp/s.tar.gz" | sha256sum -c - >/dev/null \
            && tar -xzf "$tmp/s.tar.gz" -C "$tmp" starship \
            && mv "$tmp/starship" "$out"; then
            echo "    已就位: $out"
        else
            echo "==> 警告：starship $target 获取失败，运行时将回退为在线下载/builtin 提示符"
        fi
        rm -rf "$tmp"
    done

    # Windows 版给本机 PowerShell 用：便携包应开箱即用，缺文件时 PowerShell
    # 每次启动都会报「无法识别 starship.exe」
    if [ ! -s "dist/starship/starship.exe" ]; then
        echo "==> 下载 starship v$ver (x86_64-pc-windows-msvc)..."
        local tmp
        tmp=$(mktemp -d)
        if curl -fsSL --retry 3 --retry-delay 2 --retry-all-errors "$base/starship-x86_64-pc-windows-msvc.zip" -o "$tmp/s.zip" \
            && curl -fsSL --retry 3 --retry-delay 2 --retry-all-errors "$base/starship-x86_64-pc-windows-msvc.zip.sha256" -o "$tmp/s.sha256" \
            && echo "$(awk '{print $1}' "$tmp/s.sha256")  $tmp/s.zip" | sha256sum -c - >/dev/null \
            && unzip -o "$tmp/s.zip" starship.exe -d "$tmp" >/dev/null \
            && mv "$tmp/starship.exe" "dist/starship/starship.exe"; then
            echo "    已就位: dist/starship/starship.exe"
        else
            echo "==> 警告：starship.exe（Windows）获取失败，本机 PowerShell 的 starship 提示符将不可用"
        fi
        rm -rf "$tmp"
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
fetch_starship

cp -f "$SRC/orca-term-gui.exe" dist/
cp -f "$SRC/orca-term.exe" dist/
cp -f "$CONFIG_UI_SRC/orca-term-config-ui.exe" dist/

# 随包默认 orca-config.lua（assets/orca-config.lua：本机 PowerShell 经
# starship\ 子目录启用提示符的完整配置）。部署时覆盖到程序目录即为该
# 配置；用户后续手改的配置在重新部署时需自行备份。
if [ -f "$ROOT_DIR/assets/orca-config.lua" ]; then
    cp -f "$ROOT_DIR/assets/orca-config.lua" "dist/orca-config.lua"
fi

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
