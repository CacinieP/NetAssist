# 腾讯云 CNB (Cloud Native Build) 配置指南

## 目录

1. [CNB 平台介绍](#cnb-平台介绍)
2. [快速开始](#快速开始)
3. [配置文件说明](#配置文件说明)
4. [环境变量配置](#环境变量配置)
5. [构建触发方式](#构建触发方式)
6. [监控和日志](#监控和日志)
7. [故障排查](#故障排查)

---

## CNB 平台介绍

腾讯云 **CNB (Cloud Native Build)** 是云原生 CI/CD 构建平台，提供开箱即用的持续集成和部署能力。

### 核心特性

| 特性 | 说明 |
|------|------|
| **声明式配置** | 通过 YAML 文件定义构建流程 |
| **云原生** | 基于 Docker 容器化技术 |
| **高性能** | 10秒内准备完成125G代码的构建 |
| **免费额度** | 为开源项目提供免费算力资源 |
| **国内访问快** | 相比 GitHub Actions，国内访问更稳定 |
| **集成腾讯云服务** | 原生支持 COS、SCF 等服务 |

### CNB vs GitHub Actions

| 对比项 | CNB | GitHub Actions |
|--------|-----|----------------|
| **访问速度（国内）** | ⚡ 快 | 🐌 慢 |
| **免费额度** | 1000分钟/月 | 2000分钟/月 |
| **macOS 支持** | 需配置 | 原生支持 |
| **腾讯云集成** | 原生 | 需额外配置 |
| **学习曲线** | 简单 | 中等 |

---

## 快速开始

### 1. 注册 CNB 账号

1. 访问 [https://cnb.cool](https://cnb.cool)
2. 使用 GitHub 账号授权登录
3. 创建或导入代码仓库

### 2. 配置文件

在项目根目录创建 `.cnb.yml` 配置文件（已创建）：

```yaml
main:
  push:
    - docker:
        image: node:20
      stages:
        - name: install
          script: npm ci
        - name: build
          script: npm run build
```

### 3. 提交配置

```bash
git add .cnb.yml
git commit -m "Add CNB build configuration"
git push origin main
```

推送后，CNB 会自动触发构建！

---

## 配置文件说明

### 基本结构

```yaml
分支名:
  事件名:
    - 流水线1
      stages:
        - 任务1
          script: 命令
    - 流水线2
      stages:
        - 任务2
          script: 命令
```

### 支持的分支

```yaml
main:      # main 分支
develop:   # develop 分支
feature/*: # feature 开头的分支
$          # 所有分支
```

### 支持的事件

| 事件 | 说明 | 触发时机 |
|------|------|----------|
| `push` | 代码推送 | git push |
| `pull_request` | 拉取请求 | 创建/更新 PR |
| `tag_push` | 标签推送 | git push tag |
| `manual` | 手动触发 | 在界面点击执行 |

### NetAssist 的 CNB 配置

`.cnb.yml` 已配置以下流水线：

#### 1. **main 分支 - push 事件**

```yaml
main:
  push:
    - name: build-test        # 测试流水线
      label:
        type: TEST
    - name: release-build     # 发布流水线
      ifModify:               # 文件变更检测
        - "src-tauri/**/*"
        - "src/**/*"
```

#### 2. **所有分支 - tag_push 事件（DMG 构建）**

```yaml
$:
  tag_push:
    - name: macos-dmg-release
      if: |
        [ "$CNB_REF_TYPE" = "tag" ] && [[ "$CNB_REF_NAME" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]
      stages:
        - 检查构建环境
        - 安装 Rust
        - 添加 Rust 目标架构
        - 安装 Node.js 依赖
        - 检查证书配置
        - 生成应用图标
        - 构建 x86_64 版本
        - 构建 ARM64 版本
        - 创建 Universal Binary
        - 创建 DMG 镜像
        - Apple 公证
        - 计算文件哈希
        - 上传到腾讯云 COS
```

---

## 环境变量配置

### 必需环境变量

在 CNB 项目设置中配置以下环境变量：

| 变量名 | 说明 | 获取方式 |
|--------|------|----------|
| `APPLE_CERTIFICATE` | 代码签名证书 (Base64) | 证书导出后 Base64 编码 |
| `APPLE_CERTIFICATE_PASSWORD` | 证书密码 | 导出时设置的密码 |
| `APPLE_ID` | Apple ID 电子邮件 | 您的 Apple 邮箱 |
| `APPLE_PASSWORD` | 应用专用密码 | [appleid.apple.com](https://appleid.apple.com) |
| `APPLE_TEAM_ID` | 开发团队 ID | Developer Portal 查看 |

### 可选环境变量（腾讯云 COS）

| 变量名 | 说明 |
|--------|------|
| `TENCENT_COS_SECRET_ID` | 腾讯云密钥 ID |
| `TENCENT_COS_SECRET_KEY` | 腾讯云密钥 Key |
| `TENCENT_COS_BUCKET` | COS 存储桶名称 |
| `TENCENT_COS_REGION` | COS 区域 (如 `ap-guangzhou`) |

### 配置步骤

1. 进入 CNB 项目设置
2. 找到 **环境变量** 或 **Secrets** 配置
3. 添加上述环境变量
4. 保存配置

### CNB 内置环境变量

CNB 自动注入以下环境变量：

| 变量名 | 说明 | 示例值 |
|--------|------|--------|
| `CNB_REF_NAME` | 分支名或标签名 | `main` / `v0.1.0` |
| `CNB_REF_TYPE` | 引用类型 | `branch` / `tag` |
| `CNB_COMMIT_SHA` | 提交哈希 | `abc123...` |
| `CNB_REPO_SLUG` | 仓库标识 | `user/repo` |
| `CNB_BUILD_ID` | 构建ID | `12345` |
| `CNB_TOKEN` | 访问令牌 | `xxx...` |

---

## 构建触发方式

### 方式一：自动触发（推荐）

#### 1. Push 触发

```bash
# 推送到 main 分支
git checkout main
git pull
git push origin main
```

#### 2. 标签触发（DMG 构建）

```bash
# 创建版本标签
git tag v0.1.0
git push origin v0.1.0
```

### 方式二：手动触发

1. 登录 [CNB 控制台](https://cnb.cool)
2. 进入项目页面
3. 点击 **云原生构建**
4. 选择对应流水线
5. 点击 **运行** 按钮

### 方式三：PR 检查

创建 Pull Request 时自动运行检查：

```bash
# 创建 PR
git checkout -b feature/new-feature
# ... 修改代码 ...
git push origin feature/new-feature
# 在 GitHub/Gitee 创建 PR
```

---

## 监控和日志

### 查看构建状态

1. **构建列表**
   - 进入项目 **云原生构建** 页面
   - 查看所有构建记录
   - 状态：✅ 成功 / ❌ 失败 / ⏳ 进行中

2. **构建详情**
   - 点击构建记录
   - 查看每个 Stage 的执行情况
   - 查看构建日志

### 构建日志

每个 Stage 的日志包含：

```log
================================
显示版本信息
================================
标签: v0.1.0
提交: abc123def456
分支: v0.1.0
================================

操作系统: Darwin
架构: x86_64
内核版本: 21.0.0

Rust 版本: 1.70.0
Node.js 版本: v20.0.0

✓ x86_64 版本构建完成
✓ ARM64 版本构建完成
✓ Universal Binary 创建完成
✓ DMG 创建完成: NetAssist-macos-v0.1.0.dmg
```

### 下载构建产物

构建完成后，产物可以在以下位置找到：

1. **CNB 构建页面** - 直接下载
2. **腾讯云 COS** - 持久存储
3. **GitHub Releases** - 如果配置了同步

---

## 故障排查

### 问题 1: 构建未触发

**症状**: 推送代码后 CNB 没有自动构建

**原因**:
- CNB 未关联仓库
- 分支名不匹配配置
- `.cnb.yml` 语法错误

**解决方案**:
```bash
# 检查配置文件语法
# 在 CNB 网页端查看配置校验结果

# 检查分支名
git branch --show-current

# 手动触发构建
# 在 CNB 控制台点击 "运行"
```

### 问题 2: macOS 环境不可用

**症状**: 构建日志显示 "非 macOS 构建环境"

**原因**: CNB 默认使用 Docker 容器，不是 macOS 环境

**解决方案**:

有两种方案：

**方案 A**: 使用 CNB 的 macOS Runner（需要付费或特殊权限）
```yaml
main:
  push:
    - runner:
        tags: macos  # 指定 macOS runner
      stages:
        - name: build
          script: cargo tauri build
```

**方案 B**: 使用 GitHub Actions 构建 DMG，CNB 仅做代码检查
```yaml
main:
  push:
    - docker:
        image: node:20
      stages:
        - name: lint
          script: npm run lint
        - name: test
          script: npm test
```

### 问题 3: 证书验证失败

**症状**: `code signing failed` 或 `No matching certificates`

**解决方案**:
```bash
# 检查证书格式
echo "$APPLE_CERTIFICATE" | base64 --decode | openssl pkcs12 -info

# 确认环境变量已设置
# 在 CNB 项目设置中检查环境变量

# 测试证书导入
security import certificate.p12 -k build.keychain -P "$APPLE_CERTIFICATE_PASSWORD"
```

### 问题 4: Rust 编译超时

**症状**: 构建超时失败

**解决方案**:
```yaml
# 增加超时时间
- name: 构建 x86_64 版本
  script: |
    source $HOME/.cargo/env
    cargo tauri build --target x86_64-apple-darwin
  timeout: 3h  # 增加到 3 小时
```

### 问题 5: COS 上传失败

**症状**: `coscli: command not found` 或上传失败

**解决方案**:
```bash
# 检查 COS 凭证
echo "$TENCENT_COS_SECRET_ID"
echo "$TENCENT_COS_SECRET_KEY"

# 测试 COS 连接
curl "https://${TENCENT_COS_BUCKET}.cos.${TENCENT_COS_REGION}.myqcloud.com"
```

---

## 最佳实践

### 1. 分支保护

```yaml
# main 分支需要 PR 检查
main:
  pull_request:
    - name: pr-check
      stages:
        - name: lint
          script: npm run lint
        - name: test
          script: npm test
```

### 2. 条件执行

```yaml
# 仅在特定文件变更时构建
- ifModify:
    - "src-tauri/**/*"
    - "src/**/*"
  stages:
    - name: build
      script: cargo tauri build
```

### 3. 缓存优化

```yaml
# 使用数据卷缓存
docker:
  image: node:20
  volumes:
    - /root/.npm:copy-on-write
    - node_modules:copy-on-write
```

### 4. 并行构建

```yaml
# 并行执行多个架构构建
stages:
  - name: parallel-build
    jobs:
      build-x64:
        script: cargo build --target x86_64
      build-arm64:
        script: cargo build --target aarch64
```

### 5. 失败重试

```yaml
# 失败时自动重试
- name: test
  script: npm test
  retry: 3  # 失败后重试 3 次
```

---

## 附录

### A. CNB 相关链接

- [CNB 官网](https://cnb.cool)
- [CNB 文档](https://docs.cnb.cool/zh/)
- [配置文件说明](https://docs.cnb.cool/zh/build/configuration.html)
- [语法手册](https://docs.cnb.cool/zh/build/grammar.html)
- [快速开始](https://docs.cnb.cool/zh/build/quick-start.html)

### B. 配置文件校验

在 VSCode 中配置 CNB 语法检查：

1. 安装 `redhat.vscode-yaml` 插件
2. 在 `settings.json` 添加：

```json
{
  "yaml.schemas": {
    "https://docs.cnb.cool/conf-schema-zh.json": ".cnb.yml"
  }
}
```

### C. 示例配置仓库

- [CNB 示例仓库](https://github.com/cnb-cool/examples)
- [Tauri + CNB 示例](https://github.com/tauri-apps/tauri)

---

**文档版本**: 1.0.0
**最后更新**: 2026-01-20
**状态**: ✅ 就绪

---

**需要帮助？**

- 查看 [CNB 常见问题](https://docs.cnb.cool/zh/faq.html)
- 提交 [Issue](https://github.com/cnb-cool/cnb/issues)
- 联系技术支持
