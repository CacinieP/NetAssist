# 急救箱平台验证清单

这些路径依赖各平台的系统工具，CI 只能证明代码能编译，证明不了它们在真实系统上生效。发布前请在对应系统（实体机或虚拟机）上按本清单走一遍，并把结果附在发布说明或相关 PR 里。

开始前请记下当前 DNS 配置，以便测试后还原。下文的「回读」指在 NetAssist 之外，用系统命令确认结果。

## 通用（每个平台都做）

1. 打开「断网急救」，点「开始诊断」：
   - 四项检查各自显示进度，整体在约 30 秒内结束；
   - 点「取消诊断」后按钮立即恢复，稍后到达的结果不会覆盖页面。
2. 展开「网络连接」，确认 `gateway` 与 `interface` 是真实上网用的网卡，而不是 VPN / 虚拟网卡（同时开 VPN 或插网线 + Wi-Fi 各测一次）。
3. 「手动修复工具」中，当前平台不支持的工具为灰色，并显示原因。
4. 切到其他页面再回来，诊断结果和诊断时间仍在。

## DNS：切换与恢复

在设置中把 DNS 设为一组与当前不同的服务器（例如 `223.5.5.5` / `119.29.29.29`），然后：

| 步骤 | 预期 |
|---|---|
| 手动工具 →「切换 DNS 服务器」→ 确认 | 确认框写明是持久修改、列出目标服务器；授权后提示成功 |
| 回读（见下表） | 默认网卡上是新设置的服务器 |
| `nslookup example.com` | 由新服务器应答 |
| 手动工具 →「恢复自动 DNS」→ 确认 | 提示成功 |
| 回读 | 默认网卡回到 DHCP 下发的 DNS，没有静态 DNS |
| 授权框点「取消」再试一次 | 报「授权被取消」类错误，配置不变 |

### Windows

- 回读：`netsh interface ip show dnsservers`（看默认路由所在的接口），或 `Get-DnsClientServerAddress`。
- 用以下方法确认默认接口：`Get-NetRoute -DestinationPrefix 0.0.0.0/0 | Sort-Object RouteMetric`，取 metric 最小的那条。
- 额外场景：装有 Hyper-V / WSL 虚拟交换机或 VPN 时，DNS 只应改到真实网卡上。
- 不以管理员身份运行时，应报「需要以管理员身份运行」，而不是显示成功。

### Linux

按 DNS 管理方式各测一种（发行版示例：Ubuntu 桌面 = NetworkManager；Ubuntu Server / systemd-networkd = resolved；最小化容器 = 纯 resolv.conf）。

| 管理方式 | 回读命令 | 恢复后预期 |
|---|---|---|
| NetworkManager | `nmcli -g IP4.DNS device show <网卡>`；`nmcli -g ipv4.dns,ipv4.ignore-auto-dns connection show <连接名>` | `ipv4.dns` 为空，`ignore-auto-dns` 为 `no` |
| systemd-resolved | `resolvectl status <网卡>`（看 DNS Servers、DNS Domain 是否含 `~.`、Default Route） | `resolvectl revert` 后回到原配置 |
| 纯 `/etc/resolv.conf` | `cat /etc/resolv.conf`；`ls /etc/resolv.conf.netassist-backup` | 内容还原，备份文件被删除 |

额外场景：

- 静态 IP 的 NetworkManager 连接：切换 DNS 后 IP 方式仍是手动（`ipv4.method manual`），没有被改成 DHCP。
- 通过 SSH 等无桌面会话运行（没有 polkit 认证代理）：应报「pkexec 未能完成授权……」，不能卡住。
- 一次操作只弹一次授权框。

### macOS

- 回读：`networksetup -getdnsservers <服务名>`（服务名用 `networksetup -listallhardwareports` 找默认网卡对应的那项）。
- 恢复后回读应为「There aren't any DNS Servers set」。
- 如果原本手动设置过 DNS，恢复会把它清掉；测试后请按开始前记下的配置还原。

## 结果记录模板

```text
系统 / 版本：
网络环境（有线 / Wi-Fi / VPN / 虚拟网卡）：
DNS 管理方式（仅 Linux）：
通用 1-4：通过 / 失败（说明）
DNS 切换 / 回读 / 恢复 / 取消授权：通过 / 失败（说明）
```
