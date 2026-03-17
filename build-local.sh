#!/bin/bash
# SwiftShare 本地构建脚本（Linux/macOS）
# 用于快速打包测试，无需等待 GitHub Actions

set -e

echo "=== SwiftShare 本地构建 ==="

# 检查 pnpm 是否安装
if ! command -v pnpm &> /dev/null; then
    echo "错误: 未找到 pnpm，请先安装 pnpm"
    exit 1
fi

# 检查 Rust 是否安装
if ! command -v cargo &> /dev/null; then
    echo "错误: 未找到 Rust，请先安装 Rust"
    exit 1
fi

# 清理旧的构建产物（可选）
if [[ "$*" == *"--clean"* ]]; then
    echo "清理旧的构建产物..."
    rm -rf src-tauri/target/release/bundle
    rm -f src-tauri/target/release/SwiftShare 2>/dev/null || true
fi

# 安装前端依赖
echo -e "\n[1/3] 安装前端依赖..."
pnpm install

# 构建应用
echo -e "\n[2/3] 构建应用..."
# 注意：如果你配置了签名，需要设置环境变量：
# export TAURI_SIGNING_PRIVATE_KEY="your-key"
# export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="your-password"
pnpm tauri build

# 显示构建产物位置
echo -e "\n[3/3] 构建完成！"
echo -e "\n产物位置:"

if [ -f "src-tauri/target/release/SwiftShare" ]; then
    SIZE=$(du -h "src-tauri/target/release/SwiftShare" | cut -f1)
    echo "  可执行文件: src-tauri/target/release/SwiftShare"
    echo "              大小: $SIZE"
fi

# macOS
if [ -d "src-tauri/target/release/bundle/dmg" ]; then
    find src-tauri/target/release/bundle/dmg -name "*.dmg" | while read -r dmg; do
        SIZE=$(du -h "$dmg" | cut -f1)
        echo "  DMG 安装包: $dmg"
        echo "              大小: $SIZE"
    done
fi

# Linux
if [ -d "src-tauri/target/release/bundle/deb" ]; then
    find src-tauri/target/release/bundle/deb -name "*.deb" | while read -r deb; do
        SIZE=$(du -h "$deb" | cut -f1)
        echo "  DEB 安装包: $deb"
        echo "              大小: $SIZE"
    done
fi

if [ -d "src-tauri/target/release/bundle/appimage" ]; then
    find src-tauri/target/release/bundle/appimage -name "*.AppImage" | while read -r appimage; do
        SIZE=$(du -h "$appimage" | cut -f1)
        echo "  AppImage: $appimage"
        echo "            大小: $SIZE"
    done
fi

echo -e "\n提示: 使用 --clean 参数可清理旧产物后重新构建"
echo "示例: ./build-local.sh --clean"
