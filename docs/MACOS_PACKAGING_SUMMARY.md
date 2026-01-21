# macOS DMG 打包准备完成摘要

## 已创建/更新的文件列表

### 1. GitHub Actions 工作流

| 文件 | 说明 |
|------|------|
| `.github/workflows/build-macos-dmg.yml` | 标准 GitHub Actions 工作流 |
| `.github/workflows/build-macos-tencent.yml` | 腾讯云 DevOps 优化工作流 |

### 2. 腾讯云 CNB 配置

| 文件 | 说明 |
|------|------|
| `.cnb.yml` | CNB 云原生构建配置文件 |

### 3. 配置文件

| 文件 | 说明 |
|------|------|
| `src-tauri/tauri.conf.json` | 已更新 macOS 特定配置 |

### 4. 构建脚本

| 文件 | 说明 |
|------|------|
| `scripts/build-macos.sh` | 本地构建脚本 (可执行) |

### 5. 文档

| 文件 | 说明 |
|------|------|
| `docs/MACOS_BUILD_GUIDE.md` | 完整构建指南 |
| `docs/MACOS_BUILD_QUICKSTART.md` | 快速开始指南 |
| `docs/TENCENT_CLOUD_SETUP.md` | 腾讯云配置指南 |
| `docs/CNB_SETUP_GUIDE.md` | 腾讯云 CNB 平台配置指南 |
| `docs/MACOS_PACKAGING_SUMMARY.md` | 本文件 (准备完成摘要) |

---

## 配置检查清单

在触发构建前，请确认以下配置：

### ✅ GitHub Secrets (必需)

在 GitHub 仓库 Settings > Secrets and variables > Actions 中配置：

```bash
# 必需 Secrets
APPLE_CERTIFICATE           # 证书 Base64
APPLE_CERTIFICATE_PASSWORD  # 证书密码
APPLE_ID                     # Apple ID 邮箱
APPLE_PASSWORD               # 应用专用密码
APPLE_TEAM_ID                # 开发团队 ID (10 位)

# 可选 Secrets (腾讯云 COS)
TENCENT_COS_SECRET_ID
TENCENT_COS_SECRET_KEY
TENCENT_COS_BUCKET
TENCENT_COS_REGION
```

### ✅ 配置文件更新

已在 `src-tauri/tauri.conf.json` 中添加：

```json
{
  "bundle": {
    "macOS": {
      "minimumSystemVersion": "11.0",
      "signingIdentity": "Developer ID Application: YOUR_TEAM_ID",
      "hardenedRuntime": true
    }
  }
}
```

**注意**: 请将 `YOUR_TEAM_ID` 替换为您的实际 Team ID。

---

## 构建流程图

```
┌─────────────────────────────────────────────────────────────┐
│                    触发构建                                  │
└─────────────────────────────────────────────────────────────┘
                            │
        ┌───────────┬───────────┴─────────────┬──────────────────┐
        │           │                           │                  │
        ↓           ↓                           ↓                  ↓
┌──────────┐ ┌─────────┐              ┌──────────┐      ┌──────────────┐
│ Push to  │ │  Tag    │              │ Manual   │      │     CNB      │
│ main/dev │ │ v*.*    │              │ Trigger  │      │   Platform   │
└──────┬───┘ └────┬────┘              └────┬─────┘      └──────┬───────┘
       │        │                         │                    │
       ↓        ↓                         ↓                    ↓
┌──────────────────────────────────────────────────────────────────────────┐
│              GitHub Actions / 腾讯云 CNB                                     │
├──────────────────────────────────────────────────────────────────────────┤
│  1. 环境配置                                                              │
│     └── Rust (x86_64 + ARM64)                                             │
│     └── Node.js 20                                                        │
│  2. 证书导入                                                              │
│     └── Developer ID Application                                          │
│  3. 构建                                                                  │
│     └── x86_64-apple-darwin                                               │
│     └── aarch64-apple-darwin                                              │
│  4. 创建 Universal Binary                                                  │
│  5. 创建 DMG                                                              │
│  6. Apple 公证                                                            │
│  7. 上传到 COS                                                            │
│  8. 创建 GitHub Release                                                   │
└──────────────────────────────────────────────────────────────────────────┘
                            │
        ┌───────────┬───────────┴─────────────┐
        │           │                             │
        ↓           ↓                             ↓
┌──────────────┐ ┌────────────┐           ┌──────────────┐
│ macOS DMG    │ │ GitHub     │           │ 腾讯云 COS   │
│ (Universal)  │ │ Release    │           │             │
└──────────────┘ └────────────┘           └──────────────┘
```

---

## 立即开始

### 方式 A: GitHub Actions (推荐)

```bash
# 1. 配置 GitHub Secrets (见上方配置检查清单)

# 2. 推送代码并创建标签
git add .
git commit -m "Release: v0.1.0"
git push origin main
git tag v0.1.0
git push origin v0.1.0

# 3. 等待构建完成 (约 15-20 分钟)
# 在 GitHub Actions 页面查看进度

# 4. 下载 DMG
# 从 GitHub Releases 页面下载 NetAssist-macOS-Universal.dmg
```

### 方式 B: 腾讯云 CNB (国内推荐)

```bash
# 1. 访问 https://cnb.cool 并导入/创建仓库

# 2. 在 CNB 项目设置中配置环境变量
#    (与 GitHub Secrets 相同的变量)

# 3. 推送标签触发构建
git tag v0.1.0
git push origin v0.1.0

# 4. 在 CNB 控制台查看构建进度
#    https://cnb.cool/<你的仓库>/-/pipelines

# 5. 下载 DMG
#    从 CNB 构建页面或腾讯云 COS 下载
```

### 方式 C: 本地构建

```bash
# 1. 准备环境
rustc --version  # 检查 Rust >= 1.70
node --version  # 检查 Node.js >= 18

# 2. 安装依赖并构建
npm install
chmod +x scripts/build-macos.sh
./scripts/build-macos.sh universal

# 3. 查找产物
# DMG 文件在项目根目录: NetAssist-macOS-Universal.dmg
```

---

## 文件位置参考

```
NetAssist/
├── .github/workflows/
│   ├── build-macos-dmg.yml          # GitHub Actions 标准工作流
│   └── build-macos-tencent.yml      # 腾讯云优化工作流
├── .cnb.yml                          # CNB 云原生构建配置
├── src-tauri/
│   └── tauri.conf.json              # 已更新 macOS 配置
├── scripts/
│   └── build-macos.sh                # 本地构建脚本 (已设置可执行权限)
├── docs/
│   ├── MACOS_BUILD_GUIDE.md         # 完整指南
│   ├── MACOS_BUILD_QUICKSTART.md     # 快速指南
│   ├── TENCENT_CLOUD_SETUP.md        # 腾讯云配置
│   ├── CNB_SETUP_GUIDE.md            # CNB 平台配置指南
│   └── MACOS_PACKAGING_SUMMARY.md    # 本文件
└── icon.png                           # 应用图标 (待添加)
```

---

## 首次使用前的准备

### 第一次使用前，请完成以下步骤：

1. **获取 Apple 开发者账号**
   - 访问 https://developer.apple.com/
   - 注册并支付年费 ($99)

2. **创建证书**
   - Xcode > Preferences > Accounts > Manage Certificates
   - 创建 "Developer ID Application" 证书
   - 导出为 .p12 文件

3. **转换为 Base64**
   ```bash
   base64 -i certificate.p12 > certificate_base64.txt
   ```

4. **配置 GitHub Secrets**
   - 将 Base64 证书内容添加到 `APPLE_CERTIFICATE`
   - 添加其他必需的 Secrets

5. **测试构建**
   ```bash
   git tag test-build
   git push origin test-build
   ```

6. **删除测试标签**
   ```bash
   git tag -d test-build
   git push origin :refs/tags/test-build
   ```

---

## 需要的帮助？

查看对应文档：

- **完整配置指南**: [docs/MACOS_BUILD_GUIDE.md](docs/MACOS_BUILD_GUIDE.md)
- **快速开始**: [docs/MACOS_BUILD_QUICKSTART.md](docs/MACOS_BUILD_QUICKSTART.md)
- **腾讯云 COS 配置**: [docs/TENCENT_CLOUD_SETUP.md](docs/TENCENT_CLOUD_SETUP.md)
- **腾讯云 CNB 平台**: [docs/CNB_SETUP_GUIDE.md](docs/CNB_SETUP_GUIDE.md)

---

**准备完成！现在可以开始构建 macOS DMG 了。**

---

**文档版本**: 1.0.0
**创建日期**: 2026-01-20
**状态**: ✅ 就绪
