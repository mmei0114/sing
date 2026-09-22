# Using sing

[← Overview and installation](../README.md)

sing is a terminal network proxy powered by sing-box. It manages your proxy subscriptions, routing configuration and running core. Your proxy provider, DNS servers and routing decisions remain yours to choose.

## The basic model

Traffic must enter a **listener** (an inbound) before sing-box can handle it. **Routing rules** decide what happens next: connect directly, reject, or send it to an **outbound**, such as a node or proxy group. **DNS** has its own servers and rules; importing a routing list does not automatically configure DNS.

There are three workspaces:

| Workspace | Sections |
|---|---|
| **Overview** | Health, traffic, quick group selection and recent connections |
| **Policies** | Groups, Rules, Sources |
| **Activity** | Connections, Apps, Logs |

Press `:` for a searchable **Config** popup. Core management and native modules live here, not in a fourth permanent workspace. The `,` key is an alias.

**Save is not Apply.** Editors and imports save a draft. `A` opens a review and an explicit Apply action. Applying validates the configuration, then starts a stopped core or restarts a running one; existing connections may be interrupted. Start (`s`) also loads the saved draft when the core is stopped. Live group selection and mode changes are runtime controls, not ordinary draft edits.

## First connection

1. Build and run `./sing` using the [README instructions](../README.md#get-started).
2. Open `:` → **Core**. Press `d` to fetch official releases, then `i` to open the download picker; choose a version with arrows and `Enter`. After downloading, `r` refreshes installed cores and `Enter` selects one. Runtime compatibility is tested with sing-box **1.14.0**; newer versions are not automatically guaranteed compatible.
3. Return to Overview with `Esc`, press `i`, and paste a subscription URL, share links or a local file path. Review the nodes and **Save to Draft**.
4. If the profile has not been initialized, press `s` and confirm **Upgrade saved configuration**. This adopts a native draft and creates a private backup, without starting the core. It also applies to fresh profiles in this preview.
5. Press `A` → **Apply & Start**. The initial generated configuration includes a `proxy` group for imported nodes. Select it on Overview and press `Enter` to choose its active member. Press `m` for a routing mode.
6. Choose how applications send traffic to the core, as described below.

To install the core manually, obtain the correct platform build from [official sing-box releases](https://github.com/SagerNet/sing-box/releases/tag/v1.14.0). Place the executable named `sing-box` in a directory on the manager's `PATH`, or in `bin/sing-box` inside your [private data directory](#private-data-and-recovery). Make it executable, refresh Core with `r`, and select it. If you change `PATH`, the already running manager will not inherit that change until restarted.

### Capture traffic

| Method | How it works | Things to know |
|---|---|---|
| **Proxy ports** | An app connects to a local HTTP/SOCKS listener. | Set the app's proxy to the loaded address in **Config → inbounds**. The generated mixed listener normally uses `127.0.0.1:2080`. |
| **System Proxy** | macOS advertises the local proxy through OS settings. | Use `p` or **Config → System proxy** on a local Mac. Apps that ignore OS proxy settings are unaffected. Authorization may be requested. |
| **TUN** | A native virtual-network inbound captures traffic through host routes. | Use `t`, review changes, and Apply when ready. Administrator access is required. Routing/DNS can change and SSH can be disconnected. |

Inspect the **running** capture state on Overview, not just a saved draft. “Core running,” “API ready” and “Internet reachable” are different checks. `v` on Overview offers an explicit HTTPS connectivity check.

Over SSH, all configuration and capture changes affect the remote host. Linux desktop System Proxy integration is not implemented. Do not enable TUN over your only means of reaching a machine.

### Routing modes

- **Rule:** follow the saved native routing rules, in order.
- **Global:** override traffic routing with the chosen proxy target.
- **Direct:** override traffic routing with direct connections.

Global and Direct do not delete your rules. They do not rewrite every DNS server or internal dial path: an explicitly configured DNS detour can still use a proxy in Direct mode.

## Subscriptions and groups

### Import or update nodes

Press `i` on Overview or Policies. Supported sources include share links, URL subscriptions, Clash node YAML and sing-box outbound JSON; files can be imported by path. A subscription imports nodes, not a provider's complete DNS/routing profile. Conversion takes place locally.

Check the preview and warnings before saving. In **Policies → Sources**, select a subscription and press `u` to fetch an update, then confirm the preview. Saved updates affect the draft; press `A` when ready to load them. There is no scheduled refresh in this preview.

Press `x` on a source to remove it with confirmation. Node sources remove their imported nodes; rule sources remove their corresponding rule set. If the source is still used by groups, routing, DNS or TUN, sing lists the references to change first. Deletion does not cascade into dependent groups or rules. Save affects the draft; `A` applies it to the running core.

### Create a group

Open **Policies → Groups**, press `n`, and choose **Manual** (selector) or **Automatic** (urltest). In the shared editor, set a unique `tag`, choose the `outbounds` members, and set the manual default or automatic test options. Save with `F2` or `Ctrl+S`, then Review & Apply.

Groups may contain other groups, subject to native validation. Automatic groups are selected by the core; a low latency result does not guarantee high download throughput.

On a running manual group, `Enter` opens member selection. sing confirms the selection through the API and remembers successful choices separately from the configuration's default. After a restart, remembered choices are restored only when still compatible with the group and its default; native cache-file restoration takes precedence when configured. Existing connections may keep their previous route.

When stopped, edit a group's default with `e`, or start the core to change its live selection. Use `l` for a group latency test.

## Routing rules

### Import a rule list

1. Create the target group first, if needed.
2. In Policies, press uppercase `R` and paste a URL, local path or rule text.
3. Choose the target. Review the detected format, supported-entry count and warnings, then **Save to Draft**.
4. Inspect the result in **Policies → Rules**, then `A` to Review & Apply.

The importer recognizes supported **Quantumult X, Clash, domain and IP lists**, plus native sing-box resources. It does not import another client's whole profile, scripts or DNS settings. Unsupported conditions are reported; read warnings rather than assuming conversion is exact.

Native JSON source conditions stay native. Binary `.srs` resources remain references loaded by the core; native rule-set definitions can also be edited in **Config → route → rule_set**. Remote native resource download/cache/update options belong to sing-box. Refresh converted sources with `u` in Policies → Sources.

New imported rules are placed before the first terminal routing action. Check ordering afterwards: earlier rules can take precedence. Hold `Alt` (`Option` on Mac) and press Up/Down to move the selected rule; release Alt to browse normally. Each accepted move saves to the draft and selection follows the rule. `A` reviews and applies the new order. `e` edits, `n` creates, and `x` removes with confirmation. The former `J` / `K` reorder shortcuts have been removed.

### Correct a connection's route

In **Activity → Connections**, select a connection and press `r`. Choose a supported matcher—exact domain, suffix, escaped domain regex, IP, process name, executable path, or process + domain—then select the target and review the rule in the shared editor.

You can create a rule or extend an eligible local rule with the same matcher type, action and target. sing does not append a domain to an arbitrary mixed-condition rule: native fields can be AND conditions. Remote rule-set contents are not rewritten.

Routing works with destination and process metadata, not arbitrary HTTPS URL paths. The UI is not decrypting the contents of your requests.

## Activity and application discovery

Activity shows bounded, in-memory observations obtained from the core—not a persistent packet capture or a list of every running process. Use `/` to filter and `o` to sort by recency or observed traffic. Browsing holds the reading view to keep the selected connection stable, while incoming snapshots continue to update the session history. **Held · Space live** means the view is held, not that collection stopped. Space shows the latest collected observations. Connections that begin and end between core samples can still be missed.

Pressing `3`, or `c` from Overview, opens **All connections** and clears the previous app/search scope. Entering a connection list from Apps shows that application's name and an **Esc back** hint. Left/Right section navigation leaves the app scope. The connection list uses the full width by default; `d` toggles a side detail pane in wide terminals (128 columns or more). `Enter` opens full details at any size.

In **Apps**, `Enter` shows the selected application's observed destinations. `r` creates a process-based routing correction when the necessary identity exists. Route paths come from the connection's reported chain, not from the node selected afterwards.

### Missing app names

New generated configurations enable `route.find_process`. Existing profiles are not silently rewritten:

1. Press `f` in Activity.
2. Confirm the process-discovery draft change.
3. Review and Apply.
4. Reconnect the application or open new connections.

Old observations cannot acquire missing identity retroactively. Display labels may come from macOS bundle paths; a process rule uses the actual executable name/path, not an invented bundle identifier. Shared helpers, short-lived sockets, different permissions and remote clients can prevent attribution. A shared helper is shown as the socket owner, not guessed to belong to another app.

If `f` reports **Broken pipe** after upgrading from an earlier build, restart the old manager using the upgrade steps below. Version 0.6.3-dev fixes a macOS socket-framing race; reopening the interface alone does not replace the manager.

## DNS and native configuration

Open `:` and select **dns**. Use Left/Right for **Servers**, **Rules** and **Options**:

- **Servers** define resolvers and their transports, such as local DNS, UDP or HTTPS.
- **Rules** choose how matching DNS queries are handled.
- **Options** include the final resolver and other DNS-wide fields.

DNS routing and traffic routing are separate. A site's traffic can use a proxy while its DNS uses a different path. A proxy server's own hostname also needs resolving; bootstrap/default domain resolvers and a DNS server's `detour` are distinct settings. Avoid making a resolver depend on the very proxy hostname it must resolve. A blank DNS detour uses direct dialing.

There is no universally fastest DNS preset. Change one part at a time, Apply, and test new connections on your network. See the official [DNS](https://sing-box.sagernet.org/configuration/dns/) and [route](https://sing-box.sagernet.org/configuration/route/) references for the selected core version.

Other Config modules follow the native document: `log`, `ntp`, `certificate`, `endpoints`, `inbounds`, `outbounds`, `route`, `services`, `experimental`, and **Full JSON**. Choose an item and `Enter` or `e` to edit. In an object editor, `a` reveals all documented fields and `e` opens object JSON; `F2` or `Ctrl+S` saves. `Esc` cancels or returns.

Forms retain unknown sibling fields. Full JSON covers fields without dedicated controls; this preserves JSON values, not comments or formatting. The selected core determines which fields are valid. Keep sing's protected loopback `management` service and credentials intact, or runtime control will not work.

## Keyboard reference

| Key | Action |
|---|---|
| `1` / `2` / `3` | Overview / Policies / Activity |
| Left / Right | Previous / next section |
| Arrows or `j` / `k` | Select a row |
| `Enter` / `Esc` | Open or confirm / return or cancel |
| `s` / `m` / `t` | Start/Stop / Mode / TUN |
| `p` | Local macOS System Proxy |
| `:` (or `,`) | Config module picker |
| `A` | Review & Apply |
| `i` on Overview or Policies | Import subscription |
| `n` / `e` / `x` in editable lists | New / edit / remove |
| `R` in Policies | Import rule set |
| `u` in Policies → Sources | Update selected source |
| Alt + Up / Down in Rules | Reorder selected rule (Option on Mac) |
| Tab on Overview | Switch group/connection focus |
| `c` / `v` on Overview | Open Activity / connectivity check |
| `/` / `o` in Activity | Filter / sort |
| `r` / `f` in Activity | Routing correction / app discovery |
| `d` in Activity → Connections | Toggle side details in wide terminals |
| Space in Activity | Hold the reading view / return live; collection continues |
| `F2` or `Ctrl+S` in editors | Save draft |
| `?` / `q` | Help / close the interface |

Text input and modal dialogs take precedence over workspace shortcuts. `q` leaves a running manager/core active; use `s` or `sing --shutdown` to stop it.

The `[` and `]` keys remain aliases for section navigation, without repeated on-screen hints.

## Upgrade safely

The published preview **0.6.3-dev** uses manager protocol **10**. Replacing an executable or reopening the interface does not update an already running manager. Its IPC fix requires restarting an older manager. The **0.6.4-dev** interface refinements keep protocol 10 and need only a UI reopen if the manager is already running the 0.6.3-dev IPC fix.

1. Back up your private data and current executable.
2. Close interfaces with `q`.
3. When a brief proxy interruption is acceptable, run `./sing --shutdown`. This stops the manager/core and restores managed capture settings. If restoration fails, resolve the reported error before proceeding.
4. Update/build or install the desired release, then run `./sing`. If already rebuilt, just reopen it.
5. Review the saved configuration, then Start or Apply when ready.

Use `sing` instead of `./sing` if installed on PATH, and the same `--data-dir` if you use a custom instance. Do not blindly downgrade an active private data directory; restore only a compatible backup after stopping its manager.

### Private data and recovery

Default state lives in `~/Library/Application Support/sing` on macOS and `~/.local/share/sing` on Linux; `XDG_DATA_HOME/sing` takes precedence when set. An existing legacy `sbtui` directory is reused if its corresponding `sing` directory does not exist. `--data-dir /absolute/path` selects a separate private instance.

These directories contain secrets. Git backs up source code, not your subscriptions or runtime state.

If macOS proxy settings need recovery and the interface is unavailable, use `./sing --restore-system-proxy` with the same data directory. It may request administrator authorization. Check errors rather than deleting recovery records or force-killing processes. This command is not a general repair tool for arbitrary TUN routes; retain local/console access when testing TUN.

## Privacy and reporting

Never share real subscription URLs, tokens, passwords, raw native-editor screenshots, `state.json`, `runtime.json`, private backups or sensitive connection logs. Known fields are redacted in normal snapshots/reviews, but unknown extension fields cannot all be classified as secrets. Inspect attachments yourself.

Report bugs using the [issue form](https://github.com/mmei0114/sing/issues/new/choose), including sing/core versions, OS and architecture, terminal, capture mode and reproduction steps. Prefer fictional data or the demo.

Report security issues through [private vulnerability reporting](https://github.com/mmei0114/sing/security/advisories/new). If unavailable, ask for a private contact without disclosing the vulnerability. This preview is not an audited security boundary and offers no response-time guarantee.

## Support and limits

- **Locally tested:** macOS arm64, Rust 1.94.0, sing-box 1.14.0; ordinary tests plus isolated loopback/native configuration and process-identity tests.
- **CI scope:** builds, formatting, lint, ordinary tests and the isolated manager IPC regression on macOS/Linux. See [actual workflow results](https://github.com/mmei0114/sing/actions). A green build is not real-host network acceptance.
- **Needs broader testing:** Linux/SSH networking, privileged System Proxy/TUN recovery, attribution across apps, real-provider throughput and DNS leakage behavior.
- **Not included:** Windows, Linux desktop system-proxy integration, automatic service installation, scheduled subscription refresh, complete foreign-profile conversion, HTTPS decryption or a system-wide firewall/process inventory.
- **Distribution:** build from source. There is no official Homebrew formula or crates.io package for this release; do not install an unrelated package named `sing` by assumption.

## Development

Use Git, Rust/Cargo and a C linker. Node.js 18+ is needed only for documentation scripts.

```sh
cargo test --locked
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo build --release --locked
./sing --demo
node scripts/check-docs.mjs
```

The IPC regression starts only a temporary manager and no core:

```sh
cargo test --locked --test manager manager_accepts_fragmented_request_frames -- --ignored
```

Additional explicit tests require a compatible core and local socket access:

```sh
SING_TEST_CORE=/absolute/path/to/sing-box cargo test --locked -- \
  --ignored --skip install_official_core --skip public_youtube_rule_conversion --nocapture
```

These tests use fictional temporary state, local services, a mock system-proxy helper and read-only macOS inventory. They do not enable real System Proxy or TUN. After dependencies are cached, add `--offline` if needed.

For a documentation preview from the release binary:

```sh
SING_PREVIEW_BIN="$PWD/target/release/sing" node scripts/render-preview.mjs
```

Keep changes focused and add regression tests. Preserve native fields, explicit Save/Apply behavior and cancellation/error paths. Use fictional fixtures, not real providers or private data. Describe what was tested, including untested platforms. Implementation lives in `src/`, integration tests in `tests/`, and fictional imports in `fixtures/`. This README and guide are the public documentation; working notes and generated artifacts stay out of Git.
