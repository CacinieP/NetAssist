# macOS DMG 打包快速指南

## 一键配置检查清单

在开始构建前，请确保以下事项已完成：

### 1. Apple 开发者账号
- [ ] 已注册 Apple Developer 账号 ($99/年)
- [ ] 已创建 Developer ID Application 证书
- [ ] 已记录 Team ID (10位字符)
- [ ] 已将证书导出为 .p12 文件

### 2. GitHub Secrets 配置
在 GitHub 仓库 Settings > Secrets > Actions 中添加：

| Secret 名称 | 值 |
|--------------|-----|
| `APPLE_CERTIFICATE` | 证书的 Base64 编码 |
| `APPLE_CERTIFICATE_PASSWORD` | 证书密码 |
| `APPLE_ID` | Apple ID 邮箱 |
| `APPLE_PASSWORD` | 应用专用密码 |
| `APPLE_TEAM_ID` | 开发团队 ID |

### 3. (可选) 腾讯云 COS
- [ ] 已创建 COS 存储桶
- [ ] 已创建 API 密钥
- [ ] 已配置 GitHub Secrets

---

## 快速开始

### 方式 A: 自动构建 (推荐)

```bash
# 1. 确保 main 分支代码最新
git checkout main
git pull

# 2. 创建版本标签
git tag v0.1.0
git push origin v0.1.0

# 3. 等待 GitHub Actions 自动构建完成

# 4. 下载构建产物
# 从 GitHub Releases 页面下载 NetAssist-macOS-Universal.dmg
```

### 方式 B: 本地构建

#### 前置要求
```bash
# 检查 Rust 版本
rustc --version  # 需要 1.70+

# 检查 Node.js 版本
node --version  # 需要 18+

# 检查证书
security find-identity -v "Developer ID Application"
```

#### 构建步骤

```bash
# 1. 克隆或更新项目
git pull

# 2. 安装依赖
npm install

# 3. 准备图标 (如果还没有)
# 将 1024x1024 PNG 图标保存为 icon.png
npx @tauri-apps/cli@latest icon icon.png --out src-tauri/icons

# 4. 构建应用
# 下载 macOS 构建脚本
chmod +x scripts/build-macos.sh

# 构建 Universal Binary
./scripts/build-macos.sh universal
```

#### 构建单一架构

```bash
# 仅 x86_64 (Intel)
./scripts/build-macos.sh x86_64

# 仅 ARM64 (Apple Silicon)
./scripts/build-macos.sh aarch64
```

---

## 证书导出快速指南

### 从钥匙串访问导出

1. 打开 **钥匙串访问** 应用
2. 选择 **登录** > **证书**
3. 找到 **Developer ID Application**
4. 右键点击 > **导出**
5. 文件格式: `.p12`
6. 设置密码并记住
7. 保存文件

### 转换为 Base64

```bash
# 方式一：直接输出
base64 -i certificate.p12

# 方式二：保存到文件
base64 -i certificate.p12 > certificate_base64.txt
```

---

## 应用专用密码生成

1. 访问 [appleid.apple.com](https://appleid.apple.com/)
2. 登录 Apple ID
3. 进入 **安全** > **应用专用密码**
4. 点击 **生成**
5. 输入标签: `GitHub Actions`
6. 记录生成的密码
7. 选择 **复制** 并保存

---

## 验证配置

### 测试证书

```bash
# 检查证书是否有效
security find-identity -v "Developer ID Application"

# 测试签名
codesign --force --deep --sign "Developer ID Application" test.app
```

### 测试 GitHub Actions

创建测试标签：

```bash
git tag test-build
git push origin test-build
```

验证构建是否成功后删除测试标签：

```bash
git tag -d test-build
git push origin :refs/tags/test-build
```

---

## 常见错误速查

### 错误: "No matching certificates"

**原因**: 证书未正确导入

**解决**:
```bash
security import certificate.p12 -k build.keychain -P password
```

### 错误: "notarytool: Submission failed"

**原因**: 公证失败

**解决**:
1. 检查 Apple ID 和密码
2. 确保应用专用密码正确
3. 检查网络连接

### 错误: "coscli: command not found"

**原因**: COS CLI 未安装

**解决**:
```bash
curl -o coscli https://cosbrowser.cloud.tencent.com/software/coscli-v0.9.5/mac/coscli
chmod +x coscli
```

---

## 发布流程

### 正式发布流程

1. **更新版本号**
   ```bash
   # 更新 src-tauri/Cargo.toml 和 src-tauri/tauri.conf.json 中的版本号
   ```

2. **提交更改**
   ```bash
   git add .
   git commit -m "Bump version to x.y.z"
   ```

3. **创建标签**
   ```bash
   git tag v0.1.0
   git push origin main
   git push origin v0.1.0
   ```

4. **验证构建**
   - GitHub Actions 自动触发构建
   - 等待 15-20 分钟
   - 在 Releases 页面下载 DMG

### 测试版发布流程

1. 创建 pre-release 标签：
   ```bash
   git tag v0.1.0-beta.1
   git push origin v0.1.0-beta.1
   ```

2. 在 GitHub Release 中标记为 Pre-release

---

## 后续步骤

构建完成后：

1. **下载测试**
   - 从 GitHub Releases 下载 DMG
   - 在 macOS 虚拟机中测试安装
   - 验证所有功能正常

2. **创建 GitHub Release**
   - 编辑 Release 说明
   - 添加更新日志
   - 附上安装说明

3. **通知用户**
   - 在项目 README 中添加下载链接
   - 发布更新公告
   - 发送通知给用户

---

## 需要帮助？

查看详细文档：

- [完整构建指南](docs/MACOS_BUILD_GUIDE.md)
- [腾讯云配置指南](docs/TENCENT_CLOUD_SETUP.md)
- [技术文档](TECHNICAL_REPORT.md)

遇到问题？

1. 查看 [Issues](https://github.com/xxx/netassist/issues)
2. 创建新的 Issue 并附上错误日志
3. 标签为 `macos-build` 或 `build`

---

**快速参考**

| 任务 | 命令 |
|------|------|
| 安装依赖 | `npm install` |
| 开发模式 | `npm run tauri dev` |
| 构建 | `cargo tauri build` |
| 构建特定架构 | `cargo tauri build --target x86_64-apple-darwin` |
| 本地 DMG 构建 | `./scripts/build-macos.sh universal` |
| 查看证书 | `security find-identity -v "Developer ID Application"` |

---

**版本**: 0.1.0
**更新日期**: 2026-01-20
