# NetAssist macOS DMG 打包指南

## 目录

1. [前置要求](#前置要求)
2. [Apple 开发者账号配置](#apple-开发者账号配置)
3. [代码签名证书](#代码签名证书)
4. [GitHub Secrets 配置](#github-secrets-配置)
5. [腾讯云 COS 配置](#腾讯云-cos-配置)
6. [自动化构建流程](#自动化构建流程)
7. [本地构建](#本地构建)
8. [常见问题](#常见问题)

---

## 前置要求

### 硬件要求

| 配置 | 最低要求 |
|------|----------|
| macOS 版本 | macOS 11 (Big Sur) 或更高 |
| 处理器 | Intel x86_64 或 Apple Silicon (M1/M2/M3) |
| 内存 | 4 GB RAM |
| 磁盘 | 20 GB 可用空间 |

### 软件要求

| 软件 | 版本 |
|------|------|
| Rust | 1.70+ |
| Node.js | 18+ |
| Xcode | 13+ |
| Tauri CLI | 2.1+ |

---

## Apple 开发者账号配置

### 1. 注册 Apple 开发者账号

1. 访问 [Apple Developer](https://developer.apple.com/)
2. 注册并登录（费用 $99/年）
3. 记录您的 **Team ID**（格式: `XXXXXXXXXX`）

### 2. 创建证书

#### 方法一：通过 Xcode (推荐)

1. 打开 Xcode
2. 进入 **Preferences** > **Accounts**
3. 点击 **+** 添加 Apple ID
4. 选择 **Manage Certificates**
5. 点击 **+** > **Developer ID Application**
6. 创建并下载证书（.p12 文件）

#### 方法二：通过 Developer Portal

1. 访问 [Certificates, Identifiers & Profiles](https://developer.apple.com/account/resources/certificates/list)
2. 点击 **+** > **Developer ID Application**
3. 上传 CSR 文件
4. 下载生成的证书

### 3. 创建 App ID

1. 访问 [Identifiers](https://developer.apple.com/account/resources/identifiers/list)
2. 点击 **+** > **App IDs**
3. 选择 **App** 类型
4. 输入 Bundle ID: `com.netassist.app`
5. 配置能力:
   - **App Sandbox** (可选)
   - **Hardened Runtime** (推荐)
6. 记录 **Team ID** 和 **App ID**

---

## 代码签名证书

### 导出证书为 .p12 文件

1. 打开 **钥匙串访问** (Keychain Access)
2. 找到您的 **Developer ID Application** 证书
3. 右键点击 > **导出**
4. 选择 `.p12` 格式
5. 设置密码（记住此密码）
6. 保存文件

### 转换证书为 Base64

```bash
base64 -i certificate.p12 | pbcopy
```

或者使用命令行：

```bash
base64 -i certificate.p12 > certificate_base64.txt
```

---

## GitHub Secrets 配置

### 必需的 Secrets

在 GitHub 仓库中配置以下 Secrets：

| Secret 名称 | 说明 | 获取方式 |
|--------------|------|----------|
| `APPLE_CERTIFICATE` | 代码签名证书 (Base64) | 证书导出后 Base64 编码 |
| `APPLE_CERTIFICATE_PASSWORD` | 证书密码 | 导出时设置的密码 |
| `APPLE_ID` | Apple ID 电子邮件 | 您的 Apple 邮箱 |
| `APPLE_PASSWORD` | 应用专用密码 | [appleid.apple.com](https://appleid.apple.com) |
| `APPLE_TEAM_ID` | 开发团队 ID | Developer Portal 查看 |

### 可选的 Secrets (腾讯云 COS)

| Secret 名称 | 说明 |
|--------------|------|
| `TENCENT_COS_SECRET_ID` | 腾讯云密钥 ID |
| `TENCENT_COS_SECRET_KEY` | 腾讯云密钥 Key |
| `TENCENT_COS_BUCKET` | COS 存储桶名称 |
| `TENCENT_COS_REGION` | COS 区域 (如 `ap-guangzhou`) |

### 配置步骤

1. 进入 GitHub 仓库
2. 点击 **Settings** > **Secrets and variables** > **Actions**
3. 点击 **New repository secret**
4. 添加上述 Secrets

---

## 腾讯云 COS 配置

### 1. 创建 COS 存储桶

1. 登录 [腾讯云控制台](https://console.cloud.tencent.com/)
2. 进入 **对象存储**
3. 创建存储桶：
   - 名称: `netassist-releases` (自定义)
   - 地域: 选择离用户最近的区域
   - 访问权限: **私有读写**
   - 标签: 可选

### 2. 创建密钥

1. 进入 **访问管理** > **API密钥管理**
2. 点击 **新建密钥**
3. 记录 **SecretId** 和 **SecretKey**

### 3. 配置 CORS (跨域访问)

1. 进入存储桶配置
2. 添加 CORS 规则:
   ```json
   {
     "AllowedOrigins": ["*"],
     "AllowedMethods": ["GET", "HEAD", "POST"],
     "AllowedHeaders": ["*"],
     "ExposeHeaders": ["ETag"]
   }
   ```

---

## 自动化构建流程

### 触发构建

构建流程在以下情况自动触发：

1. **Push 到分支**
   ```bash
   git push origin main
   git push origin develop
   ```

2. **创建标签**
   ```bash
   git tag v0.1.0
   git push origin v0.1.0
   ```

3. **手动触发**
   - GitHub Actions 页面 > 选择工作流 > 点击 "Run workflow"

### 构建步骤

```
1. 检出代码
2. 设置 Rust 工具链 (x86_64 + ARM64)
3. 设置 Node.js 环境
4. 安装依赖
5. 导入签名证书
6. 生成应用图标
7. 构建 x86_64 架构应用
8. 构建 ARM64 架构应用
9. 创建 Universal Binary
10. 创建 DMG 镜像
11. Apple 公证
12. 重新创建公证后的 DMG
13. 上传到腾讯云 COS
14. 创建 GitHub Release
```

### 构建时间

- **首次构建**: 约 15-20 分钟
- **增量构建**: 约 10-15 分钟

---

## 本地构建

### 1. 准备环境

```bash
# 安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 安装 Node.js
brew install node

# 安装 Tauri CLI
cargo install tauri-cli --version "^2.0.0"
```

### 2. 安装前端依赖

```bash
cd /path/to/NetAssist
npm install
```

### 3. 准备图标

创建 1024x1024 的 PNG 图标并保存为 `icon.png`：

```bash
# 放到项目根目录
cp /path/to/icon.png ./icon.png

# 生成图标
npx @tauri-apps/cli@latest icon icon.png --out src-tauri/icons
```

### 4. 构建应用

```bash
# 构建 x86_64 版本
cargo tauri build --target x86_64-apple-darwin

# 构建 ARM64 版本 (在 ARM64 Mac 上)
cargo tauri build --target aarch64-apple-darwin
```

### 5. 创建 DMG

```bash
# 构建后，在 src-tauri/target/x86_64-apple-darwin/release/bundle/macos/ 找到 NetAssist.app

# 创建 DMG
hdiutil create -volname "NetAssist" \
  -srcfolder src-tauri/target/x86_64-apple-darwin/release/bundle/macos/NetAssist.app \
  -ov -format UDRW \
  NetAssist-temp.dmg

hdiutil convert NetAssist-temp.dmg \
  -format UDZO \
  -imagekey zlib-level 9 \
  -o NetAssist.dmg

rm NetAssist-temp.dmg
```

---

## 常见问题

### Q1: 代码签名失败

**错误**: `code signing failed`

**解决方案**:
1. 检查证书是否正确导入
2. 验证 Team ID 是否正确
3. 确认证书未过期

```bash
# 检查证书
security find-identity -v "Developer ID Application"
```

### Q2: 公证失败

**错误**: `notarization failed`

**解决方案**:
1. 确认应用专用密码已生成
2. 检查 Apple ID 和密码
3. 验证网络连接

```bash
# 手动公证
xcrun notarytool submit NetAssist.zip \
  --apple-id "your@email.com" \
  --password "app-specific-password" \
  --team-id "YOUR_TEAM_ID" \
  --wait
```

### Q3: "应用已损坏" 错误

**错误**: 打开应用时提示"已损坏"

**解决方案**:
1. 确保应用已公证
2. 重新下载安装
3. 确认 Gatekeeper 设置

### Q4: 构建超时

**错误**: GitHub Actions 构建超时

**解决方案**:
1. 增加超时时间
2. 优化依赖缓存
3. 分离构建步骤

---

## 附录

### A. 快速配置清单

- [ ] 注册 Apple 开发者账号
- [ ] 创建 Developer ID Application 证书
- [ ] 导出证书为 .p12 文件
- [ ] 转换证书为 Base64
- [ ] 创建 GitHub App ID
- [ ] 配置 GitHub Secrets (5 个必需)
- [ ] 创建腾讯云 COS 存储桶
- [ ] 创建腾讯云 API 密钥
- [ ] 配置腾讯云 Secrets (可选)
- [ ] 推送代码到 GitHub
- [ ] 验证构建成功

### B. 相关链接

- [Apple Developer Portal](https://developer.apple.com/)
- [GitHub Actions 文档](https://docs.github.com/en/actions)
- [腾讯云 COS 文档](https://cloud.tencent.com/document/product/436)
- [Tauri 打包文档](https://tauri.app/v1/guides/distribution/)

---

**文档版本**: 1.0.0
**最后更新**: 2026-01-20
