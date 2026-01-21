# 腾讯云 DevOps CI/CD 配置指南

## 目录

1. [腾讯云 DevOps 介绍](#腾讯云-devops-介绍)
2. [配置腾讯云 COS](#配置腾讯云-cos)
3. [配置腾讯云 CI/CD](#配置腾讯云-cicd)
4. [手动构建和发布](#手动构建和发布)
5. [监控和日志](#监控和日志)
6. [故障排查](#故障排查)

---

## 腾讯云 DevOps 介绍

腾讯云 DevOps (CODING-DevOps) 提供了基于腾讯云的 CI/CD 解决方案，可用于自动化构建、测试和部署应用。

### 使用腾讯云 DevOps 的优势

| 优势 | 说明 |
|------|------|
| **国内访问速度快** | 相比 GitHub Actions，国内访问更稳定 |
| **集成腾讯云服务** | 原生支持 COS、SCF、VCM 等 |
| **免费额度充足** | 提供每月 1000 分钟的免费构建时间 |
| **支持 GitHub 集成** | 可直接使用 GitHub 仓库触发构建 |

---

## 配置腾讯云 COS

### 1. 创建存储桶

登录 [腾讯云控制台](https://console.cloud.tencent.com/)

1. 进入 **对象存储**
2. 点击 **创建存储桶**
3. 配置参数:
   ```
   名称: netassist-releases
   地域: 选择离用户最近的区域
     - 华南地区: ap-guangzhou (广州)
     - 华东地区: ap-shanghai (上海)
     - 华北地区: ap-beijing (北京)
   访问权限: 私有读写
   标签: 项目=NetAssist
   ```
4. 点击 **创建**

### 2. 配置跨域访问 (CORS)

1. 进入存储桶配置
2. 找到 **高级配置** > **跨域访问 CORS 设置**
3. 添加规则:

```json
{
  "AllowedOrigins": [
    "https://github.com",
    "https://gitee.com"
  ],
  "AllowedMethods": [
    "GET",
    "HEAD",
    "POST",
    "PUT"
  ],
  "AllowedHeaders": [
    "*"
  ],
  "ExposeHeaders": [
    "ETag",
    "x-cos-request-id"
  ],
  "MaxAgeSeconds": 3600
}
```

### 3. 创建子用户密钥

1. 访问 [访问管理](https://console.cloud.tencent.com/cam/cos)
2. 点击 **新建密钥**
3. 选择 **自定义密钥**
4. 记录密钥信息:
   - **SecretId**: AKIDxxxxxxxxxxxxxxxx
   - **SecretKey**: xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx

---

## 配置腾讯云 CI/CD

### 方式一：使用 GitHub Actions + COS (推荐)

GitHub Actions 支持直接上传到腾讯云 COS。

**优势**:
- 使用熟悉的 GitHub Actions
- 无需额外配置构建环境
- 支持更多触发条件

**配置步骤**:

1. 在 GitHub 仓库配置 Secrets (见 [MACOS_BUILD_GUIDE.md](MACOS_BUILD_GUIDE.md))

2. 推送代码时自动构建并上传到 COS

### 方式二：使用腾讯云 Coding DevOps

#### 创建项目

1. 访问 [腾讯云 Coding](https://coding.net/)
2. 点击 **创建项目**
3. 选择 **导入已有项目**
4. 授权 GitHub 账号

#### 配置流水线

创建 `.coding-ci.yml`:

```yaml
version: 1.0.0
name: NetAssist macOS 构建

triggers:
  push:
    branches:
      include:
        - main
        - develop
  tag:
      include:
        - v*

stages:
  - name: build
    displayName: 构建阶段
    jobs:
      - job: build_macos_dmg
        displayName: 构建 macOS DMG
        runs_on:
          macos-latest
        steps:
          - checkout: self
            with:
              fetch-depth: 0
              submodule: true
          - install@rust: 1.70.0
            with:
              targets: x86_64-apple-darwin, aarch64-apple-darwin
          - install@node: 20
          - run: npm ci
          - run: npm run build
          - script@tauri: |
              cargo install tauri-cli
              # 构建步骤...
          - publish_tencent_cos:
              coscli: coscli
              args:
                - src: NetAssist-macOS-Universal.dmg
                - dst: ${{ bucket }}/netassist/$VERSION/
```

---

## 手动构建和发布

### 1. 本地构建

详见 [MACOS_BUILD_GUIDE.md](docs/MACOS_BUILD_GUIDE.md) 的本地构建章节。

### 2. 手动上传到 COS

#### 方法一：使用 COS CLI

```bash
# 安装 COS CLI
curl -o coscli https://cosbrowser.cloud.tencent.com/software/coscli-v0.9.5/mac/coscli
chmod +x coscli

# 配置
./coscli config set -c cos.conf <<EOF
{
  "secretId": "YOUR_SECRET_ID",
  "secretKey": "YOUR_SECRET_KEY",
  "bucket": "netassist-releases",
  "region": "ap-guangzhou"
}
EOF

# 上传文件
./coscli -c cos.conf cp NetAssist-macOS-Universal.dmg \
  cos://netassist-releases/netassist/v0.1.0/NetAssist-macOS-Universal.dmg
```

#### 方法二：使用 Web 控制台

1. 进入存储桶
2. 点击 **上传文件**
3. 选择 DMG 文件上传
4. 记录文件 URL

### 3. 生成下载链接

创建下载链接的格式：

```
https://{bucket-name}.cos.{region}.myqcloud.com/{object-path}
```

示例：
```
https://netassist-releases.cos.ap-guangzhou.myqcloud.com/netassist/v0.1.0/NetAssist-macOS-Universal.dmg
```

---

## 监控和日志

### 腾讯云 Cloud Monitor

1. 进入 **云监控**
2. 创建仪表盘
3. 监控指标:
   - 存储桶使用量
   - 请求次数
   - 流量使用

### 构建日志查看

#### GitHub Actions

1. 进入 **Actions** 标签页
2. 选择对应的工作流运行
3. 查看详细日志

#### 腾讯云 Coding

1. 进入项目 **流水线**
2. 选择运行记录
3. 查看构建日志

---

## 故障排查

### 问题 1: 构建失败

**可能原因**:
- 证书未正确配置
- 依赖下载失败
- Xcode 版本过低

**解决方案**:
```bash
# 检查证书
security find-identity -v "Developer ID Application"

# 检查 Xcode 版本
xcodebuild -version

# 清理并重新构建
cargo clean
cargo tauri build --target x86_64-apple-darwin
```

### 问题 2: 上传 COS 失败

**可能原因**:
- 密钥权限不足
- 存储桶名称错误
- 网络问题

**解决方案**:
```bash
# 测试密钥
./coscli -c cos.conf ls cos://netassist-releases/

# 检查网络连接
ping ap-guangzhou.myqcloud.com
```

### 问题 3: 公证失败

**可能原因**:
- Apple ID 或密码错误
- 应用专用密码未生成
- 网络连接问题

**解决方案**:
```bash
# 检查公证状态
xcrun notarytool history

# 查看公证日志
log show --predicate 'subsystem == "com.apple.metadata.notary" --last 1h
```

---

## 附录

### A. 腾讯云 COS 区域列表

| 地区 | Region | 城市 |
|------|--------|------|
| 华南地区 | ap-guangzhou | 广州 |
| 华南地区 | ap-shenzhen | 深圳 |
| 华东地区 | ap-shanghai | 上海 |
| 华东地区 | ap-nanjing | 南京 |
| 华北地区 | ap-beijing | 北京 |
| 西南地区 | ap-chengdu | 成都 |

### B. 有用的命令

```bash
# 查看文件大小
du -sh NetAssist-macOS-Universal.dmg

# 计算文件 SHA256
shasum NetAssist-macOS-Universal.dmg

# 验证签名
codesign -dv NetAssist.app

# 查看公证状态
spctl -a -v -t execute NetAssist.app
```

### C. 相关链接

- [腾讯云 COS 文档](https://cloud.tencent.com/document/product/436)
- [腾讯云 Coding DevOps](https://coding.net/products/ci)
- [Tauri 分发指南](https://tauri.app/v1/guides/distribution/)

---

**文档版本**: 1.0.0
**最后更新**: 2026-01-20
