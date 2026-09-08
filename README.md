# NetAssist

**把网络状态、流量监控与断网排障，放进一个桌面工作台。**

[![Release](https://img.shields.io/github/v/release/CacinieP/NetAssist?style=flat-square)](https://github.com/CacinieP/NetAssist/releases/latest)
[![CI](https://github.com/CacinieP/NetAssist/actions/workflows/ci.yml/badge.svg)](https://github.com/CacinieP/NetAssist/actions/workflows/ci.yml)
[![License](https://img.shields.io/github/license/CacinieP/NetAssist?style=flat-square)](./LICENSE)
[![Stars](https://img.shields.io/github/stars/CacinieP/NetAssist?style=flat-square)](https://github.com/CacinieP/NetAssist/stargazers)

NetAssist 是基于 **Tauri 2 + Rust + React + TypeScript** 构建的网络监控与故障排查桌面工具。它把「现在连得上吗」「流量去了哪里」「连接出了什么问题」组织成几个清晰的工作界面，方便日常查看，也方便断网时逐步定位问题。

[下载安装](https://github.com/CacinieP/NetAssist/releases/latest) · [功能概览](#功能概览) · [从源码运行](#从源码运行) · [反馈问题](https://github.com/CacinieP/NetAssist/issues)

## 适合什么时候用

- **网页突然打不开**：检查网络状态，运行诊断，查看 DNS 与 HTTP 连通性结果，再选择修复操作。
- **网络变慢或流量异常**：查看实时上下行曲线、历史趋势和累计用量；macOS 还可查看应用流量排行。
- **想知道程序正在连接哪里**：按进程、协议和连接状态查看本地与远端地址、端口。
- **需要留存排障记录**：导出诊断 JSON 报告，或将流量数据导出为 CSV / JSON。

## 功能概览

| 模块 | 能做什么 |
|---|---|
| **网络仪表盘** | 查看网络连接状态、本地与公网 IP、可选的公网 IP 归属地，以及实时上下行流量曲线。 |
| **流量监控** | 查看实时速率、历史趋势、日 / 周 / 月累计流量，配置流量告警，导出 CSV / JSON 数据。 |
| **连接管理** | 查看进程名、PID、TCP / UDP 协议、本地与远端地址和端口、连接状态，并筛选连接。 |
| **断网急救箱** | 运行网络诊断，查看检测结果与修复建议，导出 JSON 诊断报告；提供 DNS 刷新、重新获取 IP、DNS 切换等修复入口。 |
| **桌面设置** | 配置刷新间隔、DNS、通知与流量阈值；支持深色模式、中英文语言设置、开机启动和关闭窗口后留在系统托盘。 |

### 平台能力与当前边界

项目包含 Windows、macOS 和 Linux 的平台实现，部分能力取决于操作系统、系统工具和权限。

| 能力 | 当前实现 |
|---|---|
| 桌面安装包 | macOS 提供 Apple Silicon / Intel 两种 DMG；Windows 提供 x64 EXE / MSI；Linux 提供 x64 DEB / AppImage。 |
| 整机流量与活动连接 | 三个平台均有对应的数据采集实现；进程信息的完整性受系统权限影响。 |
| 应用流量排行 | macOS 通过 `nettop` 采样；Windows / Linux 的进程级速率目前返回 0，不能据此判断应用没有联网。 |
| IPv6 切换、网络适配器重置 | 当前仅 macOS 实现；其他平台尚不支持这两个操作。 |
| 端口查看 | 展示已有连接及监听端口；当前没有独立的主动端口扫描功能。 |
| 结束连接 | 实际会终止连接所属的**整个进程**，可能影响该程序的其他连接及未保存工作。 |

网络修复会调用系统网络配置工具，部分操作需要管理员权限，并可能短暂中断网络。使用时请先查看诊断结果和界面中的操作说明。

## 下载安装

前往 [最新 Release](https://github.com/CacinieP/NetAssist/releases/latest)，按系统和架构选择安装包：

| 系统 | 选择的文件 | 安装方式 |
|---|---|---|
| macOS · Apple Silicon | `*_aarch64.dmg` | 打开 DMG，将应用拖入「应用程序」。 |
| macOS · Intel | `*_x64.dmg` | 打开 DMG，将应用拖入「应用程序」。 |
| Windows · x64 | `*_x64-setup.exe` 或 `*_x64_en-US.msi` | 运行安装程序并按向导完成安装。 |
| Linux · x64 | `*_amd64.deb` 或 `*_amd64.AppImage` | 使用系统包管理器安装 DEB，或给 AppImage 添加执行权限后运行。 |

macOS 构建配置的最低系统版本为 **11.0**。具体版本的安装包与更新内容以 Release 页面为准。

## 第一次使用

1. 打开**仪表盘**，确认当前网络状态、IP 信息和上下行速率。
2. 进入**流量监控**查看趋势与累计用量；有流量预算时，可配置告警阈值。
3. 进入**连接管理**查看进程与远端地址，定位需要进一步检查的连接。
4. 遇到断网时，打开**断网急救箱**运行诊断，展开检测详情，再按需要选择修复。
5. 在**设置**中调整刷新间隔、DNS、通知、外观和托盘行为。

如果关闭窗口后应用仍在运行，可从系统托盘菜单选择「退出」；也可以在设置中关闭最小化到托盘。

## 数据存储与联网行为

设置、流量历史和告警规则保存在本机的 `NetAssist` 配置目录中：

| 系统 | 默认目录 |
|---|---|
| macOS | `~/Library/Application Support/NetAssist/` |
| Windows | `%APPDATA%\NetAssist\` |
| Linux | `$XDG_CONFIG_HOME/NetAssist/`，未设置时为 `~/.config/NetAssist/` |

目录包含 `settings.json`、`alerts.json` 和 `traffic/` 下的统计数据。历史趋势来自应用实际记录的采样，不是对安装前流量的回溯。

公网 IP 查询、归属地查询和连通性检测需要访问外部服务，例如 ipify、icanhazip、ipapi、Cloudflare 等；DNS 检测会向目标 DNS 服务器发送查询。归属地显示可在设置中关闭，但关闭它不会停止其他网络检测请求。

## 从源码运行

### 开发环境

- **Node.js 与 npm**：仓库 CI 使用 Node.js 24。
- **Rust stable**：包括 Cargo；代码检查还使用 `rustfmt` 和 `clippy`。
- **Tauri 2 的系统依赖**：按系统安装原生构建工具与 WebView 依赖，参见 [Tauri 官方环境准备](https://v2.tauri.app/start/prerequisites/)。

### 启动桌面开发模式

```bash
git clone https://github.com/CacinieP/NetAssist.git
cd NetAssist
npm ci
npm run tauri -- dev
```

Tauri 会自动启动 Vite 并打开桌面窗口，前端开发服务默认使用 `1420` 端口。

`npm run dev` 只启动前端开发服务器。真实的网络数据采集、系统修复和文件导出依赖 Tauri 原生能力，完整功能请通过桌面开发模式使用。

### 构建安装包

```bash
npm run tauri -- build
```

默认构建产物位于 `src-tauri/target/release/bundle/`；指定 `--target` 时，产物位于对应目标架构的目录下。

**macOS 签名配置**：当前 `src-tauri/tauri.conf.json` 指定了维护者的 Developer ID。自行打包时，请将 `bundle.macOS.signingIdentity` 调整为自己的签名身份，或按本地构建需求移除该字段，避免因缺少对应证书而失败。

### 常用检查

以下命令与仓库 CI 的检查步骤对应：

```bash
# TypeScript 类型检查与前端生产构建
npm run build

# Rust 格式检查、静态检查与测试
cargo fmt --all --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --all-targets --all-features --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --all --manifest-path src-tauri/Cargo.toml
```

请先完成前端构建，再运行 Rust 检查，以便提供应用所需的前端资源。

## 技术栈与项目结构

| 层 | 技术 |
|---|---|
| 桌面与后端 | Tauri 2、Rust、Tokio |
| 界面 | React 18、TypeScript、Tailwind CSS |
| 图表 | ECharts |
| 状态与语言 | Zustand、i18next / react-i18next |
| 网络与系统 | reqwest、hickory-proto、sysinfo，以及平台原生工具 / API |
| 构建 | Vite、Cargo、GitHub Actions |

```text
NetAssist/
├── src/
│   ├── components/       # 仪表盘、流量监控、连接管理、急救箱与设置
│   ├── hooks/            # 网络数据与流量采样
│   ├── locales/          # 中英文语言资源
│   ├── store/            # 应用设置状态
│   └── utils/            # 通知与格式化工具
├── src-tauri/
│   ├── src/commands/     # 前端调用的 Rust 命令
│   ├── src/platform/     # Windows / Linux / macOS 实现
│   ├── src/core/         # 网络辅助逻辑
│   ├── src/models/       # 数据模型
│   └── tauri.conf.json   # 桌面窗口、安全与打包配置
├── .github/workflows/    # CI 与手动发布工作流
└── package.json
```

更多背景可参考 [技术报告](./TECHNICAL_REPORT.md)、[功能设计](./functions.md) 和 [界面设计](./UI.md)。这些文档包含设计目标，具体已实现能力以当前代码和上面的平台说明为准。

## 反馈与贡献

欢迎通过 [Issues](https://github.com/CacinieP/NetAssist/issues) 提交问题或功能建议，也欢迎提交 Pull Request。

反馈问题时，建议附上应用版本、操作系统与架构、复现步骤、预期和实际结果；涉及诊断报告或日志时，请先隐去不希望公开的 IP、进程信息等内容。

提交代码前，请运行相关的前端构建与 Rust 检查。平台相关改动请注明已验证的系统。

## 许可证

[GNU Affero General Public License v3.0（AGPL-3.0）](./LICENSE) © CacinieP
