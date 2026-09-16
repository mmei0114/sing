# sing

An English terminal client for sing-box on macOS and Linux. Runtime control uses the official sing-box 1.14+ gRPC service, not a Clash compatibility layer. Subscription and QX/Clash rule conversion happen locally.

## 0.6.0 — product workspaces

Five workspaces, visible keyboard actions, and one authoritative native configuration:

| Workspace | Pages |
|---|---|
| Overview | Status, connection setup, common groups |
| Proxies | Proxy Groups, Nodes, Subscriptions |
| Routing | Rules, Rule Sets |
| Network | Capture, Inbounds, DNS → Servers / Rules / Options |
| Activity | Connections, Logs, Diagnostics |

Settings is a global button: Core, Interface, Client, Advanced Tools. Advanced Tools keeps access to endpoints, services, experimental options and the complete native document; it does not duplicate DNS or TUN editors.

This release implements the main product workflows. The built executable has been tested on macOS arm64 with sing-box 1.14.0. **Real System Proxy/TUN, Linux/SSH host recovery and first-time user acceptance remain unverified.** See [acceptance and limitations](docs/acceptance-0.6.0.md). This is not a claim of comprehensive platform validation or improved video throughput.

## Upgrade safely

0.6.0 requires manager protocol **9**. Replacing the executable does not restart an existing manager or change networking.

1. Close old interfaces with `q`.
2. When you are ready to interrupt the current connection, run `./sing --shutdown`. This restores sing-managed proxy settings and stops the old manager/core. If restoration fails, resolve it before continuing.
3. Run `./sing`.
4. Existing native drafts remain intact. For a pre-native configuration, use the offered initialization review: a private `pre-native-state.json` backup is created before adoption.
5. Use **Review Changes**, inspect the summary / **Native Diff**, then explicitly choose **Apply**.

No application data was migrated or network takeover enabled during development. The source baseline is Git commit `b476cdb`. The previous 0.5.1 executable is backed up locally under `.build/backups/v0.5.1-f70AYn2u/sing`; generated binaries/backups are excluded from Git.

Do not downgrade an active data directory blindly. Preserve it first; stop its manager before restoring a compatible private state backup. Git contains source, not subscription credentials or runtime state.

## Run

```sh
cargo build --release --locked
./sing
./sing --demo
./sing --preview
```

The launcher prefers `target/release/sing`. `--demo` uses fictional in-memory data and never changes the network; it is not a substitute for real import/network tests. `--preview` prints a static sample. `--data-dir /absolute/path` selects an independent private instance.

Use 80 × 24 or larger; 54 × 18 is supported with scrolling. Wide terminals show a list/detail split. No mouse or special icon font is required.

## Three common tasks

### Import a subscription and connect

Choose **Import Subscription** on Overview or Proxies. Paste a subscription URL, supported node links, common Clash node YAML, sing-box node JSON, or a local path. This imports nodes, **not** another client's whole DNS/routing configuration.

Name is optional. Expand **Advanced** only if your provider requires a User-Agent.

Review → **Save & Set Up** → choose target, mode and capture → **Review Setup** → **Save Draft** → separate **Review Changes / Apply**. **Save Only** ends after importing; Overview's **Connection Setup** resumes later. Existing DNS, rules and complex listeners are not reset.

Capture choices:

- **Proxy Ports**: configure individual applications to use the local HTTP/SOCKS listener. Merely starting a core does not proxy every application.
- **System Proxy**: local macOS only; supported applications use OS proxy settings. Requires explicit authorization and a matching loopback listener.
- **TUN**: an actual native inbound, requiring administrator authorization. It can affect routes and interface DNS, and interrupt SSH. Existing TUN objects remain editable in Inbounds.
- **Keep current capture** preserves complex configurations. Switching to Ports/System does not silently delete existing TUN listeners.

Over SSH, sing controls the **remote host**, not your local computer. Linux desktop system-proxy integration is not provided.

Subscriptions has **Update**, **Update All**, preview/cancel and **Refresh Preview**. Batch updates save once; a failed source does not partially update the draft. Owned nodes with local edits or surviving references are reported as conflicts, not silently overwritten/deleted.

### Create a proxy group

Proxies → **New Group**. Set a display name, choose Manual (selector) or Automatic (urltest), search by name/source, select members, choose the manual default, then save once.

Group/endpoint members and unknown native fields are preserved. Empty groups, invalid defaults and cycles are rejected. Display names do not rename stable native tags. Automatic groups expose their test URL, interval and tolerance; lowest latency does not guarantee best throughput or streaming access.

**Select Member** controls a running manual group. sing waits for an API readback before calling it selected, and remembers successful choices separately in private `selections.json`. Runtime selection does not edit the native default or create a pending structural draft.

On Start/Apply, a remembered choice is eligible only if its group/member remain valid and the default has not changed. Automatic groups remain core-controlled. With native cache-file restoration enabled, sing leaves recovery to the core. Restoration failure is shown on Overview/Activity; cached choices are never displayed as live evidence. Existing connections may retain their old route.

When stopped, use **Edit** to change the default member, or start the core before choosing a live member.

### Import and route a rule set

Routing → **Import Rule Set** → source/format → target and insertion position → **Save Rule to Draft** → Review/Apply. **New Group** inside this flow is staged with the resource and route: cancel leaves no orphan group.

Formats:

- **auto / qx / clash / domain / ipcidr**: supported external list conditions are converted locally. Read conversion warnings before accepting exclusions; foreign policy names are not executed.
- **native**: native JSON source conditions, including logical rules, are preserved as an inline resource. No flattening through the external converter.
- **native-source / native-srs**: retain native local/remote references and source/binary formats. Auto recognizes URLs/paths ending in `.srs`. File paths are resolved on the host running sing. No rule count is invented: content is loaded by the selected core on Apply/Start.
- More native HTTP, cache, interval or other fields are available in the same resource's editor/Native JSON. Remote-resource updates belong to the core, not **Update Converted**.

Import does not change DNS. One native rule set can be used by both Route and DNS. First matching terminal routing actions and original ordering remain meaningful; nonterminal and logical rules are preserved.

## Navigation and editing

- `1`–`5`: the five workspaces. `,`: Settings.
- `Tab` / `Shift+Tab`: move focus between visible controls. Arrows choose; Enter activates. Text fields take precedence over global shortcuts.
- `/`: filter the current list. `:` or **Actions / More**: searchable actions.
- `a` Add, `e` Edit, `E` Native JSON, `x` Remove; `J/K` reorder rules.
- `F2` / `Ctrl+S` or the visible Save button: save a draft. Save is not Apply.
- **References** shows Uses / Used by; **Back to References** restores the original list/filter/selection. Removal/rename cannot leave recognized native references dangling. Atomic edits to the full native document can update definitions and references together.
- `A` / **Review Changes**: readable summary, expandable redacted Native Diff, explicit Apply. Application validates first, then restarts the core and can interrupt connections.
- `M`: Rule / Global / Direct. Global/Direct override traffic routing without deleting saved rules. DNS and internal dial paths are not implicitly rewritten; Direct is not an unconditional no-proxy guarantee for internal DNS.
- `t`: URL latency test, not a bandwidth test. `V`: core configuration check. `v` on Overview: explicit HTTPS connectivity probe.
- `q`: close only the interface; a running manager/core stays active. Stop / `d` restores managed proxy settings before stopping.

Failed submitted operations retain the original editor/preview behind an error-details view. Back does not automatically retry a write. If an apply review becomes stale, close it and request Review Changes again. Rule **Change Source** reloads current context while keeping entered source and the staged group; changed order requires position review. Subscription **Refresh Preview** re-fetches the original preview's sources.

## Native capabilities and privacy

The native JSON document is authoritative. Forms modify their fields and retain unknown siblings. Native JSON provides fields without dedicated forms; preservation is semantic JSON, not comments/whitespace. The selected core determines version/platform validity.

The protected `management` API service must retain its loopback address, port, secret and non-TLS transport. Change its port through Settings. Native profiles that remove this integration cannot be controlled by sing.

DNS Servers / Rules / Options are independent: website resolution, final DNS, bootstrap/default domain resolver and outbound detour have different roles. Blank DNS detour means direct dialing. Additional TLS, transport, FakeIP, logical/process rules and advanced inbound/outbound settings remain accessible natively; supporting an editor is not evidence of measured DNS leakage or performance.

Connections shows actual core observations, not a predicted route. Logs/Diagnostics, core/API readiness, capture and Internet checks are separate. Not all applications expose process identity.

Sources, known credentials, normal snapshots and diffs are redacted. Raw native editors intentionally expose secrets; unknown extension fields cannot be universally classified. Do not share raw-editor screenshots, `state.json`, `runtime.json` or private data backups.

## Verification and remaining limits

```sh
cargo test --offline --locked
cargo fmt --check
cargo clippy --offline --locked --all-targets -- -D warnings
SING_TEST_CORE=/absolute/path/to/sing-box cargo test --offline --locked -- \
  --ignored --skip install_official_core --skip public_youtube_rule_conversion --nocapture
```

The explicit suite uses temporary fictional data, loopback services, a MOCK system-proxy helper and read-only macOS inventory. It never enables real proxy/TUN takeover.

See [0.6.0 acceptance](docs/acceptance-0.6.0.md) for passed versus untested items. Full-profile import assistance, DNS/path timing diagnostics, subscription scheduling, dynamic group membership and hardened autostart services are outside this release. Real provider reachability, DNS leakage, throughput and device-wide takeover still need host-specific testing.
