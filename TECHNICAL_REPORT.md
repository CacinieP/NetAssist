# NetAssist 技术报告

## 项目概述

**项目名称**: NetAssist
**版本**: 0.1.0
**项目类型**: 跨平台网络监控与故障排查工具
**开发架构**: Tauri 2.x + React 18 + TypeScript
**最后更新**: 2026-01-20

---

## 目录

1. [技术栈](#技术栈)
2. [架构设计](#架构设计)
3. [功能模块](#功能模块)
4. [跨平台实现](#跨平台实现)
5. [数据流与状态管理](#数据流与状态管理)
6. [安全与性能](#安全与性能)
7. [部署与构建](#部署与构建)
8. [未来规划](#未来规划)

---

## 技术栈

### 前端技术栈

| 技术 | 版本 | 用途 |
|------|------|------|
| React | 18.3.1 | UI 框架 |
| TypeScript | 5.6.2 | 类型安全 |
| React Router | 6.26.0 | 路由管理 |
| Zustand | 5.0.0 | 状态管理 |
| Tailwind CSS | 3.4.13 | 样式框架 |
| ECharts | 5.5.0 | 数据可视化 |
| Lucide React | 0.445.0 | 图标库 |
| Vite | 5.4.8 | 构建工具 |

### 后端技术栈

| 技术 | 版本 | 用途 |
|------|------|------|
| Rust | 2021 Edition | 后端语言 |
| Tauri | 2.1 | 桌面应用框架 |
| Tokio | 1.40 | 异步运行时 |
| Serde | 1.0 | 序列化/反序列化 |
| anyhow | 1.0 | 错误处理 |
| tracing | 0.1 | 日志记录 |
| chrono | 0.4 | 时间处理 |

### 网络与系统库

| 技术 | 用途 |
|------|------|
| pnet | 0.35 | 跨平台网络接口操作 |
| trust-dns-client | 0.23 | DNS 查询 |
| reqwest | 0.12 | HTTP 客户端 |
| sysinfo | 0.32 | 系统信息获取 |
| maxminddb | 0.24 | GeoIP 数据库 |
| rusqlite | 0.32 | 数据库存储 |

---

## 架构设计

### 整体架构

```
┌─────────────────────────────────────────────────────────────────┐
│                          前端层 (React)                        │
├─────────────────────────────────────────────────────────────────┤
│  组件层                                                         │
│  ├── Dashboard          # 仪表盘总览                          │
│  ├── TrafficMonitor     # 流量监控                             │
│  ├── ConnectionManager  # 连接管理                             │
│  ├── EmergencyKit       # 断网急救                             │
│  └── Settings           # 设置管理                             │
├─────────────────────────────────────────────────────────────────┤
│  状态层 (Zustand)                                               │
│  └── settingsStore      # 设置状态管理                          │
├─────────────────────────────────────────────────────────────────┤
│  通信层 (Tauri API)                                             │
│  └── invoke()          # 前后端通信                            │
└─────────────────────────────────────────────────────────────────┘
                            ↕ Tauri IPC
┌─────────────────────────────────────────────────────────────────┐
│                       后端层 (Rust)                             │
├─────────────────────────────────────────────────────────────────┤
│  命令层 (Commands)                                              │
│  ├── ip_info            # IP信息获取                            │
│  ├── traffic            # 流量监控                              │
│  ├── traffic_history   # 流量历史                              │
│  ├── dns                # DNS检测                               │
│  ├── network_quality    # 网络质量                             │
│  ├── connections        # 连接管理                             │
│  ├── emergency          # 故障排查                             │
│  └── settings           # 设置管理                              │
├─────────────────────────────────────────────────────────────────┤
│  平台层 (Platform)                                              │
│  ├── common             # 通用工具                              │
│  ├── windows            # Windows实现                          │
│  ├── linux              # Linux实现                            │
│  └── macos              # macOS实现                            │
├─────────────────────────────────────────────────────────────────┤
│  核心层 (Core)                                                   │
│  └── models             # 数据模型                              │
└─────────────────────────────────────────────────────────────────┘
```

### 目录结构

```
NetAssist/
├── src/                          # 前端源码
│   ├── components/               # React 组件
│   │   ├── Dashboard/            # 仪表盘模块
│   │   ├── TrafficMonitor/       # 流量监控模块
│   │   ├── ConnectionManager/    # 连接管理模块
│   │   ├── EmergencyKit/         # 急救工具模块
│   │   ├── Settings/             # 设置模块
│   │   ├── StatusBar/            # 状态栏组件
│   │   └── Navigation/           # 导航栏组件
│   ├── store/                    # 状态管理
│   │   └── settingsStore.ts      # 设置存储
│   ├── App.tsx                   # 应用入口
│   └── main.tsx                  # 前端入口
│
├── src-tauri/                    # 后端源码
│   ├── src/
│   │   ├── main.rs               # 应用入口
│   │   ├── commands/             # Tauri 命令
│   │   ├── core/                 # 核心逻辑
│   │   ├── models/               # 数据模型
│   │   └── platform/             # 平台实现
│   ├── Cargo.toml                # Rust 依赖配置
│   ├── tauri.conf.json           # Tauri 配置
│   └── build.rs                  # 构建脚本
│
├── package.json                  # 前端依赖
├── tsconfig.json                 # TS 配置
├── vite.config.ts                # Vite 配置
└── tailwind.config.js            # Tailwind 配置
```

---

## 功能模块

### 1. 仪表盘 (Dashboard)

**组件位置**: `src/components/Dashboard/`

**功能描述**: 提供网络状态的整体概览

**核心功能**:
- 实时网络状态监控
- IPv4/IPv6 地址显示
- GeoIP 地理位置信息
- 实时带宽统计
- 网络延迟检测
- DNS 响应时间测试
- 活跃连接数统计
- 累计流量统计（今日/本周/本月）

**数据刷新机制**:
```typescript
// 初始加载 + 自动刷新
fetchMetrics();          // 每 2 秒刷新指标
fetchCumulative();       // 每 5 秒刷新累计流量
recordTrafficPoint();    // 每 60 秒记录流量点
```

**关键指标**:
- 总带宽 (下载 + 上传)
- 网络延迟 (HTTP 请求检测)
- DNS 响应时间
- 活跃连接数

---

### 2. 流量监控 (Traffic Monitor)

**组件位置**: `src/components/TrafficMonitor/TrafficMonitorEnhanced.tsx`

**功能描述**: 实时流量统计与应用进程流量排行

**核心功能**:

#### 2.1 实时流量监控
- 下载/上传速度实时显示
- 流量趋势图表
- 流量占比饼图

#### 2.2 应用进程流量排行
- 按应用名称排序
- 按下载流量排序
- 按上传流量排序
- 按总流量排序
- 应用搜索过滤
- 进程流量占比显示

#### 2.3 应用详情弹窗
- 应用图标与基本信息
- 实时速度统计卡片
- 累计流量统计
- 60分钟流量趋势图表

#### 2.4 总计汇总行
- 所有应用流量汇总
- 百分比进度条
- 独特视觉样式

#### 2.5 数据导出
- CSV 格式导出 (UTF-8 BOM)
- JSON 格式导出
- 包含汇总信息和详细数据
- 支持中文字段名

**数据粒度**:
```typescript
interface AppTraffic {
  name: string;                 // 应用名称
  pid: number;                  // 进程ID
  download_bytes: number;       // 累计下载字节
  upload_bytes: number;         // 累计上传字节
  current_download_bps: number; // 实时下载速度
  current_upload_bps: number;   // 实时上传速度
}
```

---

### 3. 连接管理 (Connection Manager)

**组件位置**: `src/components/ConnectionManager/ConnectionManager.tsx`

**功能描述**: 管理活跃的网络连接

**核心功能**:
- 活跃连接列表展示
- 连接详情 (协议、本地/远程地址、状态)
- 进程信息关联
- 实时刷新 (3秒间隔)
- 自动显示实际连接总数

**连接数据结构**:
```typescript
interface ConnectionInfo {
  pid: number;              // 进程ID
  process_name: string;     // 进程名称
  protocol: string;         // 协议 (TCP/UDP)
  local_address: string;    // 本地地址
  local_port: number;       // 本地端口
  remote_address: string;   // 远程地址
  remote_port: number;      // 远程端口
  state: string;           // 连接状态
}
```

---

### 4. 断网急救中心 (Emergency Kit)

**组件位置**: `src/components/EmergencyKit/EmergencyKit.tsx`

**功能描述**: 网络故障诊断与快速修复

**诊断功能**:
- 网络接口状态检测
- 网关可达性检测
- DNS 解析测试
- Internet 连接测试
- 综合诊断报告

**快速修复**:
- 刷新 DNS 缓存
- 释放并更新 IP
- 重置网络栈
- 一键修复所有问题

**修复命令映射**:
| 平台 | DNS 缓存 | IP 更新 | 网络重置 |
|------|----------|---------|----------|
| Windows | `ipconfig /flushdns` | `ipconfig /release && ipconfig /renew` | `netsh winsock reset` |
| Linux | `systemd-resolve --flush-caches` | `dhclient -r` | `systemctl restart NetworkManager` |
| macOS | `dscacheutil -flushcache` | `ipconfig set (iface) DHCP` | `killall mDNSResponder` |

---

### 5. 设置管理 (Settings)

**组件位置**: `src/components/Settings/Settings.tsx`

**功能描述**: 应用偏好配置

**设置项**:

| 设置项 | 类型 | 默认值 | 说明 |
|--------|------|--------|------|
| auto_start | bool | false | 开机自启动 |
| minimize_to_tray | bool | true | 最小化到托盘 |
| refresh_interval_secs | u32 | 1 | 刷新间隔 (1-3600秒) |
| show_geoip | bool | true | 显示 IP 地区信息 |
| primary_dns | string | "8.8.8.8" | 主 DNS |
| secondary_dns | string | "1.1.1.1" | 备用 DNS |
| notify_network_abnormal | bool | true | 网络异常通知 |
| notify_traffic_limit | bool | true | 流量超限通知 |
| traffic_limit_gb | f64 | 100.0 | 流量限制 (0.1-10000 GB) |
| dark_mode | bool | false | 深色模式 |
| language | string | "zh-CN" | 语言 |

**存储位置**:
- Windows: `%APPDATA%\NetAssist\settings.json`
- Linux: `~/.config/NetAssist/settings.json`
- macOS: `~/Library/Application Support/NetAssist/settings.json`

---

### 6. 流量告警系统

**组件位置**: `src-tauri/src/commands/traffic_history.rs`

**功能描述**: 流量使用监控与告警

**告警类型**:
- 每日下载告警
- 每日上传告警
- 每日总流量告警
- 每周流量告警
- 每月流量告警

**告警操作**:
- 添加自定义告警
- 编辑告警配置
- 删除告警
- 启用/禁用告警
- 实时告警状态检查

**告警数据结构**:
```rust
pub struct TrafficAlert {
    pub id: String,              // 告警ID
    pub name: String,            // 告警名称
    pub alert_type: String,      // 告警类型
    pub threshold_bytes: u64,    // 阈值(字节)
    pub period: String,          // 统计周期
    pub enabled: bool,           // 是否启用
    pub triggered: bool,         // 是否触发
    pub last_triggered: Option<i64>, // 最后触发时间
}
```

---

## 跨平台实现

### 平台抽象层设计

**文件位置**: `src-tauri/src/platform/mod.rs`

```rust
// 平台无关的接口定义
pub fn get_default_gateway() -> anyhow::Result<Option<IpAddr>>;
pub fn get_default_interface() -> anyhow::Result<String>;
pub fn get_network_interfaces() -> anyhow::Result<Vec<NetworkInterfaceInfo>>;
pub fn get_active_connections() -> anyhow::Result<Vec<ConnectionRawInfo>>;
pub fn flush_dns_cache() -> anyhow::Result<()>;
pub fn set_dns_servers(primary: &str, secondary: Option<&str>) -> anyhow::Result<()>;
```

### Windows 实现

**文件位置**: `src-tauri/src/platform/windows.rs`

**核心技术**:
- Win32 API `GetIfTable2` 获取接口统计
- `GetExtendedTcpTable` / `GetExtendedUdpTable` 获取连接表
- `GetAdaptersAddresses` 获取适配器信息
- Registry 读取 DNS 配置

**关键依赖**:
```toml
[dependencies]
windows = { version = "0.58", features = [
    "Win32_NetworkManagement_IpHelper",
    "Win32_NetworkManagement_Ndis",
    "Win32_System_Registry",
    "Win32_System_ProcessStatus",
]}
```

### Linux 实现

**文件位置**: `src-tauri/src/platform/linux.rs`

**核心技术**:
- `/proc/net/dev` 读取接口统计
- `/proc/net/tcp` & `/proc/net/udp` 读取连接表
- `ss -tunap` 获取连接信息
- `/etc/resolv.conf` 读取 DNS 配置

### macOS 实现

**文件位置**: `src-tauri/src/platform/macos.rs`

**核心技术**:
- `netstat -bI <iface>` 获取接口统计
- `netstat -an` 获取连接表
- `lsof -i -n -P` 获取进程信息
- `scutil --dns` 读取 DNS 配置
- `networksetup` 修改 DNS 设置
- `nettop` 获取进程流量统计

**权限要求**:
- Full Disk Access (完全磁盘访问权限) - 用于 `lsof` 和进程信息
- Network 监控权限

---

## 数据流与状态管理

### 前端状态管理 (Zustand)

**文件位置**: `src/store/settingsStore.ts`

```typescript
interface SettingsStore {
  settings: Settings;
  loading: boolean;
  saving: boolean;
  error: string | null;
  loadSettings: () => Promise<void>;
  setSettings: (partial: Partial<Settings>) => void;
  saveSettings: () => Promise<boolean>;
  resetSettings: () => Promise<void>;
}
```

**状态持久化**:
1. 应用启动时从后端加载设置
2. 设置修改后立即保存到后端
3. 后端将设置写入本地文件

### 前后端通信 (Tauri IPC)

**命令注册**:
```rust
// src-tauri/src/main.rs
.invoke_handler(tauri::generate_handler![
    commands::ip_info::get_ip_info,
    commands::traffic::get_realtime_traffic,
    commands::settings::get_settings,
    // ... 更多命令
])
```

**前端调用**:
```typescript
const stats = await invoke<TrafficStats>("get_realtime_traffic");
```

### 数据刷新策略

| 数据类型 | 刷新频率 | 优先级 |
|----------|----------|--------|
| 实时流量 | 1 秒 | 高 |
| 网络状态 | 2 秒 | 高 |
| 连接列表 | 3 秒 | 中 |
| 累计流量 | 5 秒 | 中 |
| 流量告警状态 | 5 秒 | 低 |
| IP 信息 | 5-60 秒 | 低 |

---

## 安全与性能

### 安全措施

1. **输入验证**
   - DNS 服务器地址格式验证
   - 端口范围验证 (1-65535)
   - PID 范围验证 (1-4194304)
   - 流量限制范围验证 (0.1-10000 GB)

2. **权限控制**
   - Windows: 系统关键进程保护
   - macOS: Full Disk Access 检查
   - Linux: 文件权限检查

3. **错误处理**
   - 所有命令使用 `Result<T, String>` 返回类型
   - 详细的错误日志记录
   - 用户友好的错误消息

### 性能优化

1. **前端优化**
   - 组件懒加载
   - 图表数据点限制
   - 虚拟滚动支持大量数据

2. **后端优化**
   - 异步 I/O (Tokio)
   - 连接表查询优化
   - 缓存 DNS 查询结果

3. **资源管理**
   - 自动释放网络连接
   - 图表实例及时销毁
   - 定时器正确清理

---

## 部署与构建

### 开发环境

```bash
# 安装依赖
npm install

# 启动开发服务器
npm run tauri dev

# 前端单独开发
npm run dev
```

### 生产构建

```bash
# 构建前端
npm run build

# 构建 Tauri 应用
npm run tauri build
```

### 构建产物

| 平台 | 输出位置 |
|------|----------|
| Windows | `src-tauri/target/release/netassist.exe` |
| Linux | `src-tauri/target/release/netassist` |
| macOS | `src-tauri/target/release/bundle/macos/NetAssist.app` |

---

## API 参考

### 已注册 Tauri 命令

#### IP 信息
| 命令 | 参数 | 返回值 |
|------|------|--------|
| `get_ip_info` | `include_geoip: bool` | `IPInfo` |
| `get_network_status` | - | `NetworkStatus` |

#### 流量监控
| 命令 | 参数 | 返回值 |
|------|------|--------|
| `get_realtime_traffic` | - | `TrafficStats` |
| `get_app_traffic_ranking` | - | `Vec<AppTraffic>` |

#### 流量历史
| 命令 | 参数 | 返回值 |
|------|------|--------|
| `get_cumulative_traffic` | `period: "day"\|"week"\|"month"` | `CumulativeTraffic` |
| `get_traffic_history` | `hours: number` | `TrafficHistory` |
| `record_traffic_point` | `downloadBps: number, uploadBps: number` | `bool` |
| `get_traffic_alerts` | - | `Vec<TrafficAlert>` |
| `add_traffic_alert` | `alert: TrafficAlert` | `bool` |
| `update_traffic_alert` | `alert: TrafficAlert` | `bool` |
| `delete_traffic_alert` | `alertId: string` | `bool` |
| `check_traffic_alerts` | `period: string` | `Vec<AlertStatus>` |

#### DNS 检测
| 命令 | 参数 | 返回值 |
|------|------|--------|
| `test_dns` | `server: string` | `DNSStats` |
| `get_dns_servers` | - | `Vec<String>` |

#### 网络质量
| 命令 | 参数 | 返回值 |
|------|------|--------|
| `ping` | - | `PingResult` |
| `test_http_connectivity` | `url: string \| null` | `HttpConnectivityResult` |
| `traceroute` | - | `TracerouteResult` |

#### 连接管理
| 命令 | 参数 | 返回值 |
|------|------|--------|
| `get_active_connections` | - | `Vec<ConnectionInfo>` |
| `kill_connection` | `pid: number, remote_addr: string, remote_port: number` | `bool` |

#### 故障排查
| 命令 | 参数 | 返回值 |
|------|------|--------|
| `run_diagnostics` | - | `DiagnosticResult` |
| `apply_quick_fix` | `fixType: string` | `bool` |

#### 设置
| 命令 | 参数 | 返回值 |
|------|------|--------|
| `get_settings` | - | `Settings` |
| `update_settings` | `settings: Settings` | `bool` |
| `reset_settings` | - | `Settings` |
| `check_platform_permissions` | - | `json::Value` |

#### macOS 专用
| 命令 | 参数 | 返回值 |
|------|------|--------|
| `get_macos_diagnostics` | - | `MacOSDiagnostics` |
| `get_interface_changes` | - | `InterfaceChangeEvent` |

---

## 数据模型

### Settings (设置)
```rust
pub struct Settings {
    pub auto_start: bool,
    pub minimize_to_tray: bool,
    pub refresh_interval_secs: u32,
    pub show_geoip: bool,
    pub primary_dns: String,
    pub secondary_dns: String,
    pub notify_network_abnormal: bool,
    pub notify_traffic_limit: bool,
    pub traffic_limit_gb: f64,
    pub dark_mode: bool,
    pub language: String,
}
```

### TrafficStats (流量统计)
```rust
pub struct TrafficStats {
    pub download_bps: number,
    pub upload_bps: number,
    pub timestamp: number,
}
```

### AppTraffic (应用流量)
```rust
pub struct AppTraffic {
    pub name: string,
    pub pid: number,
    pub download_bytes: number,
    pub upload_bytes: number,
    pub current_download_bps: number,
    pub current_upload_bps: number,
}
```

### ConnectionInfo (连接信息)
```rust
pub struct ConnectionInfo {
    pub pid: number,
    pub process_name: string,
    pub protocol: string,
    pub local_address: string,
    pub local_port: number,
    pub remote_address: string,
    pub remote_port: number,
    pub state: string,
}
```

---

## 未来规划

### 短期计划 (v0.2.0)
- [ ] 添加网络使用历史趋势图表
- [ ] 实现流量使用预测
- [ ] 添加网络速度测试功能
- [ ] 支持多语言切换 (中英日韩等)
- [ ] 添加深色模式主题

### 中期计划 (v0.3.0)
- [ ] 实现流量限制功能
- [ ] 添加进程防火墙规则
- [ ] 支持网络流量包捕获
- [ ] 添加定期报告生成
- [ ] 实现数据备份/恢复

### 长期计划 (v1.0.0)
- [ ] 分布式监控支持
- [ ] 移动端配套应用
- [ ] 云端数据同步
- [ ] 高级分析与告警
- [ ] 企业级管理后台

---

## 开发贡献

### 代码规范

**Rust 代码**:
- 使用 `cargo fmt` 格式化
- 使用 `cargo clippy` 检查代码
- 遵循 Rust 命名规范

**TypeScript 代码**:
- 使用 ESLint 检查代码
- 使用 Prettier 格式化
- 遵循 React Hooks 规范

### 测试

```bash
# 运行 Rust 测试
cd src-tauri
cargo test

# 运行前端测试
npm test
```

---

## 许可证

MIT License

---

## 联系方式

- 项目仓库: [GitHub]
- 问题反馈: [Issues]

---

*本文档生成日期: 2026-01-20*
*文档版本: 1.0.0*
