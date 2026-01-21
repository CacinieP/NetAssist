#!/bin/bash

# NetAssist macOS 本地构建脚本
# 使用方法: ./scripts/build-macos.sh [universal|x86_64|aarch64]

set -e

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# 日志函数
log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

# 检查系统要求
check_requirements() {
    log_info "检查系统要求..."

    # 检查 macOS 版本
    MACOS_VERSION=$(sw_vers -productVersion)
    MIN_VERSION="11.0"

    if [ "$(printf '%s\n' "$MIN_VERSION" "$MACOS_VERSION" | sort -V | head -n1)" != "$MIN_VERSION" ]; then
        log_success "macOS 版本: $MACOS_VERSION ✓"
    else
        log_error "macOS 版本过低，需要 $MIN_VERSION 或更高版本"
        exit 1
    fi

    # 检查架构
    ARCH=$(uname -m)
    log_info "系统架构: $ARCH"

    # 检查 Rust
    if ! command -v cargo &> /dev/null; then
        log_error "Rust 未安装，请访问 https://rustup.rs/"
        exit 1
    fi
    log_success "Rust 版本: $(rustc --version)"

    # 检查 Node.js
    if ! command -v node &> /dev/null; then
        log_error "Node.js 未安装"
        exit 1
    fi
    log_success "Node.js 版本: $(node --version)"

    # 检查 npm
    if ! command -v npm &> /dev/null; then
        log_error "npm 未安装"
        exit 1
    fi
}

# 安装依赖
install_dependencies() {
    log_info "安装前端依赖..."
    npm ci
    log_success "前端依赖安装完成"
}

# 检查证书
check_certificates() {
    log_info "检查代码签名证书..."

    IDENTITY=$(security find-identity -v "Developer ID Application" 2>/dev/null | head -1)

    if [ -z "$IDENTITY" ]; then
        log_warn "未找到 Developer ID Application 证书"
        log_info "请按照以下步骤配置："
        echo "1. 打开 Xcode"
        echo "2. Preferences > Accounts"
        echo "3. 点击 'Manage Certificates'"
        echo "4. 创建 'Developer ID Application' 证书"
        echo ""
        read -p "按 Enter 继续（假设证书已配置）..."
    else
        log_success "找到证书: $IDENTITY"
    fi
}

# 检查图标
check_icons() {
    log_info "检查应用图标..."

    ICONS_DIR="src-tauri/icons"

    if [ ! -f "$ICONS_DIR/icon.icns" ]; then
        log_warn "未找到 macOS 图标"

        if [ -f "icon.png" ]; then
            log_info "从 icon.png 生成图标..."
            npx @tauri-apps/cli@latest icon icon.png --out "$ICONS_DIR"
            log_success "图标生成完成"
        elif [ -f "assets/icon.png" ]; then
            log_info "从 assets/icon.png 生成图标..."
            npx @tauri-apps/cli@latest icon assets/icon.png --out "$ICONS_DIR"
            log_success "图标生成完成"
        else
            log_error "未找到图标文件 (icon.png 或 assets/icon.png)"
            log_info "请创建一个 1024x1024 的 PNG 图标并保存为 icon.png"
            exit 1
        fi
    else
        log_success "图标文件存在"
    fi
}

# 清理旧的构建产物
clean_build() {
    log_info "清理旧构建产物..."
    rm -rf "build-universal"
    rm -f "NetAssist-temp.dmg"
    rm -f "NetAssist-macOS-Universal.dmg"
    log_success "清理完成"
}

# 构建应用
build_app() {
    local TARGET=$1

    log_info "开始构建应用 (目标: $TARGET)..."
    cargo tauri build --target "$TARGET"

    BUILD_PATH="src-tauri/target/$TARGET/release/bundle/macos/NetAssist.app"

    if [ -d "$BUILD_PATH" ]; then
        log_success "应用构建成功: $BUILD_PATH"
    else
        log_error "应用构建失败"
        exit 1
    fi
}

# 创建 Universal Binary
create_universal_binary() {
    log_info "创建 Universal Binary..."

    BUILD_DIR="build-universal"
    mkdir -p "$BUILD_DIR"

    # 复制 x86_64 应用
    X64_APP="src-tauri/target/x86_64-apple-darwin/release/bundle/macos/NetAssist.app"
    ARM64_APP="src-tauri/target/aarch64-apple-darwin/release/bundle/macos/NetAssist.app"

    if [ ! -d "$X64_APP" ]; then
        log_error "x86_64 应用不存在，请先构建"
        exit 1
    fi

    cp -R "$X64_APP" "$BUILD_DIR/"

    # 合并二进制文件
    if [ -d "$ARM64_APP" ]; then
        log_info "合并 x86_64 和 ARM64 二进制..."
        lipo -create \
            "$BUILD_DIR/NetAssist.app/Contents/MacOS/NetAssist" \
            "$ARM64_APP/Contents/MacOS/NetAssist" \
            -output "$BUILD_DIR/NetAssist.app/Contents/MacOS/NetAssist-universal"

        mv "$BUILD_DIR/NetAssist.app/Contents/MacOS/NetAssist-universal" \
           "$BUILD_DIR/NetAssist.app/Contents/MacOS/NetAssist"

        log_success "Universal Binary 创建完成"
    else
        log_warn "ARM64 应用不存在，使用仅 x86_64 版本"
    fi

    # 重新签名
    log_info "重新签名应用..."
    codesign --force --deep --sign "Developer ID Application" "$BUILD_DIR/NetAssist.app"
    log_success "重新签名完成"
}

# 创建 DMG
create_dmg() {
    log_info "创建 DMG 镜像..."

    BUILD_DIR="build-universal"
    APP_NAME="NetAssist"
    DMG_NAME="${APP_NAME}-macOS-Universal"

    # 创建临时目录
    TMP_DIR=$(mktemp -d)

    # 复制应用
    cp -R "$BUILD_DIR/${APP_NAME}.app" "$TMP_DIR/"

    # 创建 DMG
    hdiutil create -volname "$APP_NAME" \
        -srcfolder "$TMP_DIR" \
        -ov -format UDRW \
        "${DMG_NAME}-temp.dmg"

    # 转换为压缩格式
    hdiutil convert "${DMG_NAME}-temp.dmg" \
        -format UDZO \
        -imagekey zlib-level 9 \
        -o "${DMG_NAME}.dmg"

    # 清理
    rm -rf "${DMG_NAME}-temp.dmg" "$TMP_DIR"

    log_success "DMG 创建完成: ${DMG_NAME}.dmg"
}

# 公证应用
notarize_app() {
    log_info "公证应用..."

    BUILD_DIR="build-universal"

    # 打包应用
    zip -r "${BUILD_DIR}/NetAssist-app.zip" "${BUILD_DIR}/NetAssist.app"

    # 上传公证
    log_info "上传到 Apple 公证服务..."
    xcrun notarytool submit "${BUILD_DIR}/NetAssist-app.zip" \
        --apple-id "$APPLE_ID" \
        --password "$APPLE_PASSWORD" \
        --team-id "$APPLE_TEAM_ID" \
        --wait

    # 装订公证票据
    log_info "装订公证票据..."
    xcrun stapler staple "${BUILD_DIR}/NetAssist.app"

    # 清理
    rm "${BUILD_DIR}/NetAssist-app.zip"

    log_success "公证完成"
}

# 主函数
main() {
    local TARGET=${1:-universal}

    echo "================================"
    echo "  NetAssist macOS 构建脚本"
    echo "================================"
    echo ""

    # 检查构建类型
    case "$TARGET" in
        universal|x86_64|aarch64)
            ;;
        *)
            log_error "无效的构建目标: $TARGET"
            echo "支持的目标: universal, x86_64, aarch64"
            exit 1
            ;;
    esac

    # 如果是 universal，需要两个架构
    if [ "$TARGET" = "universal" ]; then
        log_info "构建 Universal Binary (x86_64 + ARM64)"
        build_app x86_64-apple-darwin
        build_app aarch64-apple-darwin
        create_universal_binary
    else
        log_info "构建单一架构: $TARGET"
        build_app "$TARGET-apple-darwin"
        BUILD_DIR="src-tauri/target/$TARGET-apple-darwin/release/bundle/macos"
        BUILD_DIR="build-universal"
        mkdir -p "$BUILD_DIR"
        cp -R "src-tauri/target/$TARGET-apple-darwin/release/bundle/macos/NetAssist.app" "$BUILD_DIR/"
    fi

    # 创建 DMG
    create_dmg

    # 询问是否公证
    if [ -n "$APPLE_ID" ] && [ -n "$APPLE_PASSWORD" ] && [ -n "$APPLE_TEAM_ID" ]; then
        read -p "是否进行公证？(y/N) " -n 1 -r
        echo
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            notarize_app
            # 重新创建公证后的 DMG
            create_dmg
        fi
    else
        log_warn "跳过公证（需要配置 APPLE_ID, APPLE_PASSWORD, APPLE_TEAM_ID）"
    fi

    echo ""
    echo "================================"
    log_success "构建完成！"
    echo "================================"
    echo ""
    echo "产物位置:"
    echo "  - 应用: build-universal/NetAssist.app"
    echo "  - DMG:  NetAssist-macOS-Universal.dmg"
    echo ""
}

# 执行主函数
main "$@"
