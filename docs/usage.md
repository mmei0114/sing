# sing user guide

[← Project overview](../README.md)

An English terminal client for sing-box, locally tested on macOS arm64 with Linux as a target platform. Runtime control uses the official sing-box 1.14+ gRPC service, not a Clash compatibility layer. Subscription and QX/Clash rule conversion happen locally.

## 0.6.1-dev — quiet shell and evidence-based routing

Three primary workspaces and one authoritative native configuration:

| Workspace | Pages |
|---|---|
| Overview | Status, connection setup, common groups |
| Policies | Proxy Groups, ordered Rules, node/rule Sources |
| Activity | Connections, observed Apps and links, Logs |

The fixed bottom controls are Start/Stop, Rule/Global/Direct Mode and TUN. Config is a global utility and follows sing-box's document order: Core, `log`, `dns`, `ntp`, `certificate`, `endpoints`, `inbounds`, `outbounds`, `route`, `services`, `experimental`, and full JSON. DNS Servers / Rules / Options use the same list-and-object editor language as every other native object.

Activity keeps session history from the core's connection API. Connections and observed apps can be sorted by recency or traffic. `r` creates a prefilled native rule from the selected evidence: exact domain, domain suffix, escaped regex, process name, exact process path, or process + domain. The shared editor is always the final review, and the new rule is inserted before the first terminal routing rule. This is not a system-wide process inventory: applications with no captured and identified connection do not appear.

This release implements the main product workflows. The built executable has been tested on macOS arm64 with sing-box 1.14.0. **Real System Proxy/TUN, Linux/SSH host recovery and first-time user acceptance remain unverified.** See [acceptance and limitations](acceptance-0.6.0.md). This is not a claim of comprehensive platform validation or improved video throughput.

## Upgrade safely

0.6.0 requires manager protocol **9**. Replacing the executable does not restart an existing manager or change networking.

1. Close old interfaces with `q`.
2. When you are ready to interrupt the current connection, run `./sing --shutdown`. This restores sing-managed proxy settings and stops the old manager/core. If restoration fails, resolve it before continuing.
3. Run `./sing`.
4. Existing native drafts remain intact. For a pre-native configuration, use the offered initialization review: a private `pre-native-state.json` backup is created before adoption.
5. Use **Review Changes**, inspect the summary / **Native Diff**, then explicitly choose **Apply**.

Back up your own executable and private data before upgrading. Generated binaries, local backups and runtime state are excluded from Git; cloning the repository does not restore your private configuration.

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

### Private data and recovery

Default private data lives in `~/Library/Application Support/sing` on macOS and `~/.local/share/sing` on Linux. If `XDG_DATA_HOME` is set, its `sing` subdirectory is used instead. An existing legacy `sbtui` directory is reused when the corresponding `sing` directory does not exist. `--data-dir` overrides this location. These directories contain secrets; keep backups private and stop the manager before restoring compatible state.

For normal macOS system-proxy control, use `p` on Overview. If the interface/manager cannot be used, `./sing --restore-system-proxy` is the dedicated recovery command; it may request administrator authorization. Use the same `--data-dir` as the affected instance. Inspect reported errors rather than deleting recovery records or force-killing processes. The system-proxy recovery command does not repair arbitrary TUN routes; keep local/console access when testing TUN. Actual host-level recovery remains a known validation gap.

## Three common tasks

### Import a subscription and connect

Press `i` on Overview or Policies. Paste a subscription URL, supported node links, common Clash node YAML, sing-box node JSON, or a local path. This imports nodes, **not** another client's whole DNS/routing configuration.

Name is optional. Expand **Advanced** only if your provider requires a User-Agent.

Review → **Save & Set Up** → choose target, mode and capture → **Review Setup** → **Save Draft** → separate **Review Changes / Apply**. Apply starts a stopped core or restarts a running one; a second Start is not needed after success. **Save Only** ends after importing; Overview's **Connection Setup** resumes later. Existing DNS, rules and complex listeners are not reset.

Capture choices:

- **Proxy Ports**: configure individual applications to use the local HTTP/SOCKS listener. Inspect the actual address/port in Config → `inbounds`; the generated mixed listener defaults to `127.0.0.1:2080`. Use the loaded configuration, not an unapplied draft value. Merely starting a core does not proxy every application.
- **System Proxy**: local macOS only; supported applications use OS proxy settings. Requires explicit authorization and a matching loopback listener.
- **TUN**: an actual native inbound, requiring administrator authorization. It can affect routes and interface DNS, and interrupt SSH. Existing TUN objects remain editable in Inbounds.
- **Keep current capture** preserves complex configurations. Switching to Ports/System does not silently delete existing TUN listeners.

Over SSH, sing controls the **remote host**, not your local computer. Linux desktop system-proxy integration is not provided.

Subscriptions has **Update**, **Update All**, preview/cancel and **Refresh Preview**. Batch updates save once; a failed source does not partially update the draft. Owned nodes with local edits or surviving references are reported as conflicts, not silently overwritten/deleted.

### Create a proxy group

Policies → **Groups** → `n`. Set a display name, choose Manual (selector) or Automatic (urltest), choose members and the manual default, then save once.

Group/endpoint members and unknown native fields are preserved. Empty groups, invalid defaults and cycles are rejected. Display names do not rename stable native tags. Automatic groups expose their test URL, interval and tolerance; lowest latency does not guarantee best throughput or streaming access.

**Select Member** controls a running manual group. sing waits for an API readback before calling it selected, and remembers successful choices separately in private `selections.json`. Runtime selection does not edit the native default or create a pending structural draft.

On Start/Apply, a remembered choice is eligible only if its group/member remain valid and the default has not changed. Automatic groups remain core-controlled. With native cache-file restoration enabled, sing leaves recovery to the core. Restoration failure is shown on Overview/Activity; cached choices are never displayed as live evidence. Existing connections may retain their old route.

When stopped, use **Edit** to change the default member, or start the core before choosing a live member.

### Import and route a rule set

Policies → `R` **Import Rule Set** → source preview → target → **Save to Draft** → Review/Apply. Imported rules are inserted after nonterminal preparation actions and before the first terminal routing action.

Formats:

- **auto / qx / clash / domain / ipcidr**: supported external list conditions are converted locally. Read conversion warnings before accepting exclusions; foreign policy names are not executed.
- **native**: native JSON source conditions, including logical rules, are preserved as an inline resource. No flattening through the external converter.
- **native-source / native-srs**: retain native local/remote references and source/binary formats. Auto recognizes URLs/paths ending in `.srs`. File paths are resolved on the host running sing. No rule count is invented: content is loaded by the selected core on Apply/Start.
- More native HTTP, cache, interval or other fields are available in the same resource's editor/Native JSON. Remote-resource updates belong to the core, not **Update Converted**.

Import does not change DNS. One native rule set can be used by both Route and DNS. First matching terminal routing actions and original ordering remain meaningful; nonterminal and logical rules are preserved.

## Navigation and editing

- `1`–`3`: Overview / Policies / Activity. `,`: Config.
- `s`: Start/Stop. `m`: Rule / Global / Direct. `t`: TUN. `p` on Overview: local macOS System Proxy. `A`: Review & Apply.
- Arrows or `j/k` choose; Enter activates. Text fields take precedence over global shortcuts.
- `[` / `]`: previous/next section. Each workspace keeps its selected row.
- `i`: import a subscription. `n` in Groups/Rules: new item. `R`: import a rule set. `e`: edit. `x`: remove. `J/K`: reorder rules.
- `o` in Activity toggles recent/traffic ordering. `r` creates a rule from the selected connection or app. Enter on an app shows its links.
- `F2` / `Ctrl+S` in the shared editor saves a draft. Save is not Apply; `a` reveals all documented fields and `e` opens object JSON.
- **References** shows Uses / Used by; **Back to References** restores the original list/filter/selection. Removal/rename cannot leave recognized native references dangling. Atomic edits to the full native document can update definitions and references together.
- `A`: opens **Review & Apply**, with a readable summary and explicit confirmation. Application validates first, then restarts the core and can interrupt connections.
- `m`: Rule / Global / Direct. Global/Direct override traffic routing without deleting saved rules. DNS and internal dial paths are not implicitly rewritten; Direct is not an unconditional no-proxy guarantee for internal DNS.
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

See [0.6.0 acceptance](acceptance-0.6.0.md) for passed versus untested items. Full-profile import assistance, DNS/path timing diagnostics, subscription scheduling, dynamic group membership and hardened autostart services are outside this release. Real provider reachability, DNS leakage, throughput and device-wide takeover still need host-specific testing.
