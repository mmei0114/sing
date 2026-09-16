# sing — 原生 sing-box 终端客户端

**订阅、代理分组与分流规则，在一个终端里完成。**

[![CI](https://github.com/mmei0114/sing/actions/workflows/ci.yml/badge.svg)](https://github.com/mmei0114/sing/actions/workflows/ci.yml)
[![Version](https://img.shields.io/badge/version-0.6.0-4c8bf5)](https://github.com/mmei0114/sing/releases/tag/v0.6.0)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)

[English](README.md) · 简体中文

sing 是一个使用 Rust / Ratatui 构建的 **sing-box TUI**：为日常代理操作提供交互式终端界面，同时保留 sing-box 原生配置的表达能力。导入订阅、选择节点、创建代理组、导入规则集并绑定出口，这些常用操作不必从手写 JSON 开始；需要精细控制时，可以继续深入原生配置。

不依赖 Clash 兼容 API，不使用第三方订阅转换服务。**遵循 sing-box 的逻辑，让它更容易使用。**

![sing 实际 Demo 界面：三个克制的工作区、实时流量、代理组，以及固定的 Start、Mode、TUN 控制](docs/assets/overview.svg)

*图片来自程序实际 `sing --preview` 输出，使用虚构数据，不代表真实代理连接。*

[快速开始](#快速开始) · [使用手册（英文）](docs/usage.md) · [参与贡献](CONTRIBUTING.md) · [反馈问题](https://github.com/mmei0114/sing/issues/new/choose)

> **早期版本：**当前本地验证平台为 macOS Apple Silicon。Linux 是目标平台，真实 Linux/SSH 网络以及 System Proxy/TUN 特权接管恢复尚未完成验收。请先阅读[测试范围与限制](docs/acceptance-0.6.0.md)，不要直接用于唯一的关键网络连接。sing 是独立项目，非 SagerNet 官方客户端。

## 能做什么

- **导入与更新订阅**：支持常见节点链接、订阅 URL、Clash 节点 YAML、sing-box 节点 JSON；更新前先预览，支持单个或全部来源更新。
- **一次创建代理组**：搜索、多选成员，选择手动 selector 或自动 urltest，设置默认成员后一次保存。支持合法的嵌套组引用。
- **导入规则并完成分流**：本地转换受支持的 QX、Clash、域名及 IP 规则列表；也能保留原生 Source/SRS 资源。目标组、规则位置和内联建组在同一流程里完成，不支持的条目会明确报告。
- **独立配置 DNS**：DNS Servers / Rules / Options 各有明确职责；导入路由规则不会偷偷重写 DNS。
- **保存与应用分开**：先保存草稿，再查看变化摘要或脱敏原生差异，最后明确 Apply。不会因为修改一个字段就自动重启内核。
- **查看实际运行情况**：节点切换经过 API 回读确认；查看连接、日志和诊断，区分内核运行、系统接管与网站访问检测。
- **从实际连接修正分流**：Activity 按应用和目标整理内核观测到的连接，可按时间或流量排序，并把选中连接转成可编辑的域名、正则、进程、进程路径或 App + Domain 规则。

运行控制使用 sing-box 1.14+ 官方 gRPC 服务；当前验收内核为 **1.14.0**，不保证未来版本自动兼容。手动组的成功选择可以在 Apply 后恢复，同时不改写原生默认成员。

## 快速开始

### 编译并体验

需要 Git、Rust/Cargo 和终端。已测试 Rust **1.94.0**；可按 [Rust 官方说明](https://www.rust-lang.org/tools/install)安装。macOS 编译缺少链接器时需安装 Xcode Command Line Tools；Linux 需要 C 链接器等基础构建工具。

```sh
git clone https://github.com/mmei0114/sing.git
cd sing
cargo build --release --locked
./sing --demo
```

Demo 只使用内存中的虚构数据，不会启动代理或改变网络。按 `q` 退出，再打开真实客户端：

```sh
./sing
```

想直接使用 `sing` 命令，可以在仓库中运行 `cargo install --path . --locked`，并确保 Cargo 的安装目录在 PATH 中。本版本没有官方 Homebrew formula，也未发布到 crates.io；不要把其他同名软件当作本项目。

### 第一次连接

1. 按 `,` 打开 **Core**，下载或选择兼容的 sing-box 内核。内置下载会校验官方发布摘要；仓库不捆绑内核。
2. 在 Overview 按 `i`，粘贴订阅来源，审阅识别结果并保存到草稿。
3. 到 **Policies → Groups** 选择代理；`m` 切换 Rule / Global / Direct，`t` 控制 TUN，Overview 的 `p` 控制 macOS 本机 System Proxy。
4. 按 `A` 审阅并明确 Apply。Apply 会启动已停止的内核，或重启正在运行的内核。

**启动内核不代表所有软件自动走代理。** SSH 中运行 sing 操作的是远程主机，不是你面前的电脑；不要在唯一的关键 SSH 通道中随意测试 TUN。

使用 Proxy Ports 时，到 `,` **Config → inbounds** 查看实际监听地址和端口。程序生成的 mixed 入站默认 `127.0.0.1:2080`，同时支持 HTTP/SOCKS；在应用代理设置里填写你自己的实际值，该地址不是网页。如果自动下载内核不可用，可从[官方发行页](https://github.com/SagerNet/sing-box/releases/tag/v1.14.0)取得匹配架构的可执行文件，再到 Core 选择。

断开代理用 **Stop**；`q` 仅关闭界面，后台内核会继续运行。已有版本升级请先看[安全升级步骤](docs/usage.md#upgrade-safely)，不要盲目覆盖活动数据目录。

## 功能在哪里

| 工作区 | 内容 |
|---|---|
| Overview | 运行状态、连接引导、常用代理组 |
| Policies | Proxy Groups / 有序 Rules / 订阅与规则 Sources |
| Activity | Connections / 已观测 Apps 与链接 / Logs / 快速分流修正 |

Config 是全局入口，按 sing-box 原生顶层配置顺序组织：`log`、`dns`、`ntp`、`certificate`、`endpoints`、`inbounds`、`outbounds`、`route`、`services`、`experimental`，最后是完整 JSON；Core 信息、选择和下载也在这里。TUI 界面目前仅英文，中文文档不代表存在中文界面。

方向键选择，`Enter` 执行，`Esc` 返回。不在输入框时，`1`–`3` 切换工作区，`[` / `]` 切换区段，`,` 打开 Config。底部稳定保留 `s` Start/Stop、`m` Mode 和 `t` TUN。页面动作旁直接显示键位，输入框优先接收文字。建议终端至少 **80×24**，无需鼠标或 Nerd Font。

## 使用前要知道

- sing 是客户端，sing-box 才是转发流量的代理内核。节点及其服务由你自己提供。
- 这里的“Clash/QX 导入”指节点或受支持的规则列表，**不是完整配置迁移**；不会整体转换对方的 DNS、脚本、重写等功能。
- Rule 使用保存的原生路由；Global/Direct 是明确的运行时覆盖，不删除你的规则，也不隐式改写 DNS 和内部拨号路径。
- 暂无 Linux 桌面系统代理集成、Windows 支持、订阅定时更新或自动安装后台服务。高级字段可编辑，不代表每个功能已在每个平台验收。
- 延迟测试不等于带宽或视频体验测试；不承诺使用后一定提速。
- **不要在 Issue、截图或日志中公开订阅链接、Token、原始配置、私有备份。** 安全问题请看 [Security](SECURITY.md)。

详细说明见[使用手册](docs/usage.md)、[验收台账](docs/acceptance-0.6.0.md)、[更新记录](CHANGELOG.md)。欢迎提供 Linux/SSH 实测、转换器虚构测试样本、交互改进或可复现的问题；贡献方式见 [Contributing](CONTRIBUTING.md)。

## 致谢与许可

感谢 [sing-box](https://github.com/SagerNet/sing-box)、[Ratatui](https://github.com/ratatui/ratatui) 与 [Crossterm](https://github.com/crossterm-rs/crossterm)。终端交互及文档组织参考了 [Lazygit](https://github.com/jesseduffield/lazygit)、[fzf](https://github.com/junegunn/fzf)、[bat](https://github.com/sharkdp/bat)，不代表这些项目对 sing 的背书或合作。

sing 采用 [MIT License](LICENSE)。sing-box 为独立项目，遵循[其自身许可证](https://github.com/SagerNet/sing-box/blob/main/LICENSE)；依赖项保留各自许可。
