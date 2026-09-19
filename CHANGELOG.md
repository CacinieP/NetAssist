# 更新日志

本文件记录 NetAssist 的所有重要变更。格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)。

## [未发布]

### 🐛 修复

- **流量阈值告警恢复工作**：`check_traffic_alerts` 的前端调用没有传 `period`，而 Tauri 对缺失的必需参数直接拒绝（`missing required key period`），该命令自 #22 起一直调用失败——告警状态卡片为空、阈值通知也从未触发。后端参数改为 `Option<String>`（仍然忽略，仅兼容旧 UI），调用方无需再传值。
- **告警检测跟随应用生命周期**：流量阈值检测与通知从 `TrafficMonitorEnhanced` 的 `useEffect` 提升为 App 级 `useTrafficAlertMonitor`（`src/hooks/useTrafficAlertMonitor.ts`）。此前切到仪表盘或设置页即卸载页面、轮询被清理，用户离开流量页期间的阈值触发完全收不到通知；现在 5 秒轮询持续运行，页面只负责展示百分比。
- **告警去重语义修正**：去重集合改为模块级并新增「首轮基线」——应用启动时已经处于触发状态的告警属于已知状态，不再每次启动都弹一次通知；解除后再次触发仍会通知（第三处通知为新增告警的首次触发）。

### 🔧 变更

- **全局网络轮询增加并发保护**：`useNetworkData` 的共享轮询现在单次执行（慢请求不再堆积、节拍不再塌缩），并用「配置代次」校验丢弃过期结果——切换刷新间隔/GeoIP 开关或停止轮询后，旧请求的结果不会再覆盖新快照（这同时保证「网络异常」跃迁通知基于最新状态）。
- **实时流量轮询与历史写入同样增加在途保护**：`get_realtime_traffic` 与 `record_traffic_point` 均改为单次执行，避免请求重叠导致的速率尖刺与重复写入。

### 📦 依赖 / CI

- 新增 `.github/workflows/audit.yml`：仅在依赖清单/锁文件/审计配置变化时（以及每周一次）运行 `cargo audit`，信息级发现不阻断、新漏洞阻断。
- 新增 `src-tauri/.cargo/audit.toml`：仅对 RUSTSEC-2026-0194 / RUSTSEC-2026-0195（quick-xml 0.37.5）记录例外，并在注释中写明依据与解除条件（#34）——本应用只经过 `tauri-winrt-notification 0.7.2` 的 `quick_xml::escape::escape()` 使用 quick-xml，两个公告涉及的 `NsReader`/重复属性检查解析路径不在调用链上；待 notify-rust 脱离 0.7.x 后删除例外。

## [0.3.7] — 2026-09-17

### ✨ 新增

- **流量导出支持自选时间范围**：导出前可选范围（今日 / 本周 / 本月 / 全部历史，最长覆盖 45 天保留窗口），CSV 与 JSON 内容包含导出元信息（范围、起止时间戳、采样点数）与**带时间戳的历史采样序列**；此前仅导出当前快照、范围不可选。后端新增 `get_export_traffic_history` 命令，导出路径不受图表 14 天钳制限制。
- **运行日志持久化**：日志按天滚动写入配置目录 `logs/` 子目录（如 `~/Library/Application Support/NetAssist/logs/netassist.log.2026-09-17`），非阻塞写入不影响界面流畅度；自动保留最近 7 天，过期文件在启动时与每日翻篇时清理。此前打包版日志仅输出到 stdout（被系统丢弃），排障信息无从获取；现在反馈问题可直接附上对应日期的日志文件。debug 构建同时保留终端输出，日志级别约定不变（debug 构建 DEBUG / release 构建 INFO）。

### 🔧 变更

- 新增 `tracing-appender` 依赖；日志初始化迁移至 `src-tauri/src/logging.rs` 模块（含保留清理的单元测试与后台清理线程）。

## [0.3.6] — 2026-09-17

### ✨ 新增

- **流量导出附带 OS 网卡级计数**：JSON / CSV 导出新增 `os_download_bytes` / `os_upload_bytes` / `os_total_bytes` 字段（CSV 对应「OS接口下载/上传/总字节」三行）。数据来自默认网卡的累计字节计数（新增 `get_interface_counters` 命令，复用既有 `get_interface_total_bytes` 平台实现，macOS / Windows / Linux 三端一致）。即使 nettop 拿不到每进程数据，导出也有可靠的 OS 层基线可对账。

### 🔧 变更

- **nettop 诊断日志**：nettop 执行失败、输出为空、解析出 0 条数据块时输出 warn 级警告（含 Full Disk Access 权限、macOS 版本兼容性提示），便于排查「每进程流量为空」的问题；正常解析计数降为 debug 级，避免每 3 秒轮询刷爆 release（INFO 级）日志。

### 📦 其他

- 同步 `package-lock.json` 版本号至 0.3.5（前两次发版遗漏）。

## [0.3.5] — 2026-09-16

### 📦 依赖

- RustSec 修复：cargo update crossbeam-epoch / h2 / rustls；hickory-proto 0.25 → 0.26、plist 1.9 → 1.10（#33）。

## [0.3.4] — 2026-09-16

### 🐛 修复

- macOS 应用图标按 Apple 图标网格规范补齐内边距（824/1024）。
- autostart 开启后注入 LaunchAgent KeepAlive（#30）。
- 清理仓库根目录误提交的 32×32 占位 icon.ico；npm audit 修复（14 → 5 条）。

## [0.3.3] — 2026-09-16

### 🔧 变更

- 降低常驻占用：缩短历史数据保留、电池供电时降频采样、GeoIP 默认关闭（#31）。

## [0.3.2] — 2026-09-08

### 🐛 修复

- 修复审计 issue #12–#24（P0/P1/P2）（#26）。
- 重建 GitHub Actions（改为手动 workflow_dispatch 发版）+ 流量监控页修复（#11）。

## [0.3.1] — 2026-07-07

### 🐛 修复

- 修复流量监控页「只有实时流量有数字」：累计流量改用 OS 接口字节计数器 + 磁盘锚点（`traffic/anchors.json`），打开页面立即显示今日/本周/本月真实流量，并自动处理周期翻篇与接口重置（Wi-Fi 切换/重启）；流量告警与历史趋势图随之恢复。

[0.3.7]: https://github.com/CacinieP/NetAssist/compare/v0.3.6...v0.3.7
[0.3.6]: https://github.com/CacinieP/NetAssist/compare/v0.3.5...v0.3.6
[0.3.5]: https://github.com/CacinieP/NetAssist/compare/v0.3.4...v0.3.5
[0.3.4]: https://github.com/CacinieP/NetAssist/compare/v0.3.3...v0.3.4
[0.3.3]: https://github.com/CacinieP/NetAssist/compare/v0.3.2...v0.3.3
[0.3.2]: https://github.com/CacinieP/NetAssist/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/CacinieP/NetAssist/commits/v0.3.1
