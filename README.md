# sing — a native sing-box TUI

**Your subscriptions, proxy groups and routing rules. One terminal.**

[![CI](https://github.com/mmei0114/sing/actions/workflows/ci.yml/badge.svg)](https://github.com/mmei0114/sing/actions/workflows/ci.yml)
[![Version](https://img.shields.io/badge/version-0.6.0-4c8bf5)](https://github.com/mmei0114/sing/releases/tag/v0.6.0)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)

English · [简体中文](README.zh-CN.md)

sing is an independent, keyboard-first **terminal UI for [sing-box](https://sing-box.sagernet.org/)**, built in Rust with Ratatui. Import subscriptions, choose a proxy, create groups, and bind rule sets without writing JSON for everyday tasks. When you need more control, edit the underlying native configuration without losing fields the forms do not expose.

**Native sing-box semantics. A more approachable client.** No Clash compatibility API or third-party subscription-conversion service is required.

![sing Overview rendered from the real demo: three quiet workspaces, live traffic, proxy groups and fixed Start, Mode and TUN controls](docs/assets/overview.svg)

*Actual `sing --preview` output, rendered as SVG. Fictional demo data; no live traffic.*

[Get started](#get-started) · [Features](#what-you-can-do) · [User guide](docs/usage.md) · [Contribute](CONTRIBUTING.md) · [Report a bug](https://github.com/mmei0114/sing/issues/new/choose)

> **Early release:** macOS Apple Silicon is the locally tested build. Linux is a target platform; real Linux/SSH networking and privileged System Proxy/TUN recovery are not yet acceptance-tested. Review the [verification record](docs/acceptance-0.6.0.md) before relying on sing for critical connectivity. This project is not an official SagerNet client.

## What you can do

- **Bring your subscriptions.** Import supported node links, subscription URLs, Clash node YAML or sing-box node JSON. Preview updates before saving; update one source or all sources together.
- **Build groups in one step.** Search and select members, choose a manual selector or automatic latency-testing group, then save once. Nested groups and native defaults remain available.
- **Route by rule set.** Convert supported Quantumult X (QX), Clash, domain and IP lists locally, or keep native Source/SRS resources. Choose the target group and rule position in the same flow. Unsupported entries are reported, not silently accepted.
- **Keep DNS independent.** Manage DNS servers, DNS rules and options in one place. Importing a routing rule set does not rewrite your DNS settings.
- **Review before applying.** Read an object-level summary or a redacted native diff. Saving a draft does not restart the core; Apply is explicit.
- **See what is actually running.** API-confirmed group choices, observed connections, logs and diagnostics distinguish core readiness, traffic capture and Internet checks. Successful manual selections can survive Apply without overwriting group defaults.
- **Correct a route from evidence.** Activity groups observed network connections by application and destination. Sort by recency or traffic, then turn a connection into an editable domain, regex, process, process-path or App + Domain rule.

Runtime control uses the official sing-box 1.14+ gRPC service. Compatibility has been tested with **1.14.0**; future core releases are not automatically guaranteed compatible.

## Get started

### Build and try it

You need Git, a Rust toolchain with Cargo, and a terminal. Rust **1.94.0** is the tested toolchain; install Rust using the [official instructions](https://www.rust-lang.org/tools/install). On macOS, install Xcode Command Line Tools if a linker is missing; Linux needs a C linker/build toolchain.

```sh
git clone https://github.com/mmei0114/sing.git
cd sing
cargo build --release --locked
./sing --demo
```

The demo uses fictional, in-memory data and never starts a proxy or changes your network. Exit with `q`, then launch the real client:

```sh
./sing
```

Prefer a command on your PATH? From the cloned repository, use `cargo install --path . --locked` and then `sing`. There is no official Homebrew formula or crates.io installation in this release. Do not assume an unrelated package named `sing` is this project.

### Make your first connection

1. Press `,` and open **Core**, then download or select a compatible sing-box executable. The installer verifies the official release digest; the core is not bundled with this repository.
2. On Overview, press `i`, paste a subscription source, review the detected nodes, and save it to the draft.
3. Open **Policies → Groups** to choose a proxy. Use `m` for Rule / Global / Direct, `t` for TUN, or `p` on Overview for local macOS System Proxy.
4. Press `A`, review the changes, then explicitly apply. Applying starts a stopped core or restarts a running one.

Starting a core alone does **not** proxy every application. Over SSH, sing manages the **remote host**, not your local computer. Do not try TUN takeover over your only critical SSH connection.

For Proxy Ports, find the loaded listener under `,` **Config → inbounds**. The generated mixed listener defaults to `127.0.0.1:2080` and accepts HTTP or SOCKS; use your actual configured address/port in the application's proxy settings. That port is a proxy endpoint, not a website. If core download is unavailable, obtain the matching architecture from [official sing-box releases](https://github.com/SagerNet/sing-box/releases/tag/v1.14.0) and select its executable in Core.

Use **Stop** to disconnect. `q` closes the interface but leaves a running core active. Upgrading an existing installation? Read the [safe upgrade steps](docs/usage.md#upgrade-safely) first.

## Find your way around

| Workspace | What belongs here |
|---|---|
| **Overview** | Connection status, setup and common groups |
| **Policies** | Proxy Groups · ordered Rules · subscription and rule Sources |
| **Activity** | Connections · observed Apps and links · Logs · quick routing fixes |

Config is global and follows the native sing-box document: `log`, `dns`, `ntp`, `certificate`, `endpoints`, `inbounds`, `outbounds`, `route`, `services`, `experimental`, then full JSON. Core installation and selection live alongside that tree without duplicating native objects.

Arrows select, `Enter` activates, and `Esc` goes back. `1`–`3` switch workspaces, `[` / `]` switch sections, and `,` opens Config. The stable bottom controls are `s` Start/Stop, `m` Mode and `t` TUN. Keys are shown beside page actions; text input takes priority. Use an **80×24 or larger** terminal; no mouse or Nerd Font is required.

## A few important distinctions

**Is this a new proxy engine?** No. sing manages sing-box; sing-box handles traffic. Your provider and nodes are still your own.

**Does it import entire Clash or QX configurations?** No. Node subscriptions and supported rule lists are imported separately. Their DNS, scripts and other application settings are not translated wholesale. See [formats and native resources](docs/usage.md#import-and-route-a-rule-set).

**Does Rule / Global / Direct replace sing-box routing?** No. Rule uses your saved native rules; Global/Direct apply explicit runtime overrides without deleting them. DNS and internal dial paths are not silently rewritten.

**Are all platforms and advanced features verified?** No. Native fields remain accessible, but a field editor is not proof of platform support. There is no Linux desktop system-proxy integration, Windows support, subscription scheduler or automatic service installation in this release. Latency tests do not measure download speed. See [known limits](docs/acceptance-0.6.0.md#remaining-limitations).

## Documentation and contributing

- [User guide](docs/usage.md): subscriptions, groups, routing, DNS, native editing and recovery.
- [Verification and limits](docs/acceptance-0.6.0.md): what was tested and what still needs real-host acceptance.
- [Contributing](CONTRIBUTING.md): build, test and submit reproducible reports or focused changes.
- [Security](SECURITY.md): private reporting and what never to include in public logs or screenshots.
- [Changelog](CHANGELOG.md): release notes.

Useful contributions include Linux/SSH testing, clearer first-run interactions, converter fixtures and reproducible bug reports. **Never post subscription URLs, tokens, raw native configurations or private backups in an issue.**

## Built with—and inspired by

[sing-box](https://github.com/SagerNet/sing-box) provides the proxy engine; [Ratatui](https://github.com/ratatui/ratatui) and [Crossterm](https://github.com/crossterm-rs/crossterm) provide the terminal foundation. [Lazygit](https://github.com/jesseduffield/lazygit), [fzf](https://github.com/junegunn/fzf) and [bat](https://github.com/sharkdp/bat) inspired the focus on discoverable terminal workflows and concise documentation. These projects do not sponsor or endorse sing.

## License

sing is licensed under [MIT](LICENSE). sing-box is a separate project with its [own license](https://github.com/SagerNet/sing-box/blob/main/LICENSE); third-party dependencies retain their respective licenses.
