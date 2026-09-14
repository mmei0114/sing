# sing

A native terminal client for sing-box on macOS and Linux. Rust + Ratatui; runtime control uses the official sing-box 1.14+ gRPC service. No Clash core or online conversion service is required.

## 0.5.0 — native configuration workspace

The native JSON document is now authoritative. Forms edit individual subtrees of that same document; fields not edited by a form are preserved. Node subscriptions and converted rule-list metadata live outside that document. Resource refreshes update their owned objects, not the entire configuration.

The UI is English-only; names in subscriptions and user-created objects retain their original language.

### 0.5.1 — top navigation and visible actions

Top tabs replace the sidebar, with direct page keys and compact actions shown on each page. Outbounds has a `g` group shortcut; import and conversion controls are visible under Resources. Advanced lists only sections without dedicated pages, while `E` still opens the complete document. This UI-only update keeps manager protocol 6: from 0.5.0, close the interface with `q` and reopen `./sing`; no shutdown or migration is needed.

This is the first implementation of the new architecture, not a claim that every native field has a dedicated form or that all platforms have been validated. The selected core validates the configuration before application. Version 1.14.0 is the tested baseline; feature availability also depends on platform and core build.

## Upgrade from 0.4.1

The local Git tag `v0.4.1-baseline` preserves the old source. The old macOS arm64 executable is separately preserved at `.build/backups/v0.4.1/sing` (excluded from Git).

1. Close old sing interfaces with `q`.
2. Run `./sing --shutdown`. This restores managed system proxy settings and stops the old core. If restoration fails, resolve that error before upgrading.
3. Run `./sing` again. The new UI requires manager protocol **6**; it never silently restarts an old manager.
4. On Overview, press `u` to review the native migration, then Enter to accept it. A private `pre-native-state.json` backup is created in the application data directory before schema 2 is saved. Migration alone does **not** start/restart a core or change host networking.
5. Review your configuration and press `A` for **Review & Apply**. Enter confirms application; the core performs validation first.

The previous routing policy and DNS are materialized into editable native objects once. Subsequent routing/DNS edits are independent. A fresh empty configuration includes an empty default selector: import nodes before using that selector, or remove it and choose a valid direct-only configuration.

Do not open the schema-2 data directory with old versions. For a downgrade: stop the new manager, preserve the new data separately, and restore the private `pre-native-state.json` as `state.json` before launching the backed-up 0.4.1 executable. Do not copy active sockets or delete live runtime state. Git is a source backup, not a credential/data backup.

## Build and run

```sh
cargo build --release --locked
./sing
./sing --demo
./sing --preview
```

`--demo` uses fictional, in-memory data and performs no network operations. `--preview` prints a static terminal preview. Minimum terminal size: 54 × 18; top tabs wrap to fit the terminal. At 115 columns, pages show a list/detail split.

The `./sing` launcher prefers `target/release/sing`. Rebuild release after changing the code, or run the debug binary explicitly during development.

## Navigation

```text
1 Overview   2 Inbounds   3 Outbounds   4 Routing   5 DNS
6 Resources  7 Advanced   8 Connections 9 Logs      0 Diagnostics  - Settings
```

- `1`–`9`, `0`, `-`: open the corresponding top tab directly. `Tab` focuses navigation; left/right chooses a page, Enter returns to content. Arrows or `j`/`k` move within lists. Navigation never replaces page content.
- `[` / `]`: switch a page's subpages. `/`: filter; Esc clears the filter.
- `a`: add; `e`: form; `E`: native JSON; `x`: remove. `Enter`: details or a selector's member picker.
- `g` in Outbounds: create a manual or automatic group. Overview `a` and Resources / Subscriptions `a`: import a node subscription. Resources / Rule sets `C`: import and convert a QX/Clash rule list, choosing its routing target in the same form.
- `F2` / `Ctrl+S`: save the draft. Esc cancels the current editor. Space or left/right cycles choices; Space opens a member picker.
- `A`: Review & Apply; `V`: core validation; `p`: redacted effective configuration preview.
- `c`: Start, only when stopped; `d`: Stop and restore managed proxy settings; `q`: close the UI without stopping the manager/core.
- `?`: contextual help. `M`: explicit traffic-routing override.

Structural changes remain draft changes until Apply. Application restarts the core and interrupts connections. A loaded selector's member can be changed through gRPC without restarting the entire core. Its selected member is also persisted in the native draft. Edited/new group membership must be applied before selecting a member not loaded by the core.

### Inbounds and system integration

Manage multiple native listeners. Forms cover local mixed listeners and TUN addresses, stack, route settings, interface DNS mode and MTU. Use native JSON for additional listener types/fields. Listener authentication is native `users` JSON; do not expose an unauthenticated proxy publicly.

`s` opens system integration. `port` means no system-proxy takeover; `system` enables macOS proxy management against the specified local mixed-proxy port. The matching unauthenticated loopback mixed inbound must exist in the document. This port also serves the client's explicit connectivity check and subscription-download proxy fallback. System integration does not rewrite listeners.

TUN is determined by the document's inbounds, independently of macOS system-proxy integration. TUN requires explicit administrator authorization and can interrupt SSH. In 1.14, TUN `dns_mode` defaults to `hijack`, which includes platform interface-DNS configuration where available. It is incorrect to assume TUN never changes native DNS settings. Application-owned DoH may follow another path.

Linux has no automatic desktop system-proxy integration in this release. SSH always controls the host on which sing runs, not the local computer displaying the SSH session. Linux/TUN/SSH recovery and dual-stack behavior still require dedicated real-host acceptance testing. The existing TUN helper is experimental, not a guaranteed crash-safe network recovery service.

### Outbounds

Nodes, `direct`, `selector` and `urltest` groups share one list. Group membership may reference other groups/endpoints; cycles and missing members are rejected before application. Names are shown in pickers while native tags remain the references. Changing a tag does not automatically rename every reference: repair references before Apply.

`t` requests a native URL latency test for the selected loaded outbound. Results appear beside outbounds when returned by the core. URLTest groups use their native URL/interval/tolerance; the default template tests gstatic every three minutes while running. Latency is not bandwidth or streaming-unlock capability.

Native JSON retains protocol-specific TLS, transport, multiplexing and dial settings. The form does not silently normalize or discard fields it does not expose. Advanced fields are not generic speed switches.

### Routing and DNS

Routing has one ordered native rules list and an Options subpage for `final`, interface detection and the default resolver for server hostnames. `J`/`K` moves a rule. Ordinary match fields accept comma-separated lists; logical rules retain nested native conditions. Additional conditions/actions remain editable through JSON.

DNS has **Resolvers / Rules / Options**. Add named local, UDP, TCP, TLS, HTTPS, QUIC, HTTP/3 or FakeIP resolvers. Configure server IP/hostname, custom port/path, outbound detour and bootstrap resolver. Blank detour means direct dialing; do not select an empty direct outbound as a DNS detour. DNS rules and final resolver are independent of website routing. Cache, timeout, IP preference, optimistic caching and reverse mapping have option fields; richer values survive form round trips.

`M` exposes Rule / Global / Direct as client traffic-routing overrides:

- Rule runs the original native route rules.
- Global/Direct replace traffic routing decisions, retaining sniff/DNS-hijack actions and an optional explicit private-IP exception.
- **DNS servers/rules and their outbound dependencies are unchanged.** Direct is therefore not a promise that internal DNS traffic avoids all proxy nodes. Review explains this before Apply. No automatic fallback to direct is added when a node fails.

The DNS template tests validate syntax with the actual 1.14 core; they do not measure resolver reachability, DNS leakage, video performance or throughput. FakeIP configuration support is not a claim of tested end-to-end FakeIP behavior on each platform.

### Resources

Subscriptions: `a` imports a URL, supported node URI, pasted content or local file; `r` previews refresh; Enter confirms and Esc cancels. URI/base64, common Clash YAML node entries and sing-box node JSON are converted locally. This does not adopt a foreign full configuration's DNS/routes.

Rule sets: `a` adds native inline/local/remote JSON or SRS references; `C` converts a common QX/Clash/domain/CIDR list and appends a native routing rule to the chosen target. Review unsupported entries before accepting partial conversion. Complex native rule sets should be used natively, not sent through the simplified cross-format converter.

Native remote resources use sing-box's own HTTP client/update/cache settings. Converted resources are fetched/updated by sing with a confirmation preview, using direct download first and the running local proxy as fallback. `r` refreshes converted resources without rewriting DNS or existing native routing decisions. Native remote resources update according to their configured core policy; `r` is not a forced native-resource refresh operation.

Subscription updates never silently overwrite a locally modified node object. Rename its native tag to detach the local copy, then refresh the source. Existing nonempty selector membership is explicit, not automatically expanded or repaired after node removals. Broken references must be resolved before applying. Removing/detaching a converted native rule set also removes its conversion metadata; native routing references remain visible for repair.

### Advanced, diagnostics and privacy

Advanced lists sections without dedicated pages (such as endpoints, services and experimental options); DNS, Routing, Inbounds and Outbounds are not duplicated there. `E` edits the entire document as strict JSON, including TLS, cache and other version-supported capabilities. Unknown fields and ordered arrays are preserved on save. This is semantic JSON preservation, not preservation of comments or whitespace. Invalid drafts may be saved for further editing; Apply runs reference checks and `sing-box check` before stopping a running instance.

The `management` API service is a protected integration point: keep its loopback address, non-TLS transport and secret. Change its port through Settings, which updates the matching native field. Other native services remain untouched. A complete externally sourced config requires retaining this service before sing can manage it.

Native editing exposes credentials intentionally. Normal snapshots, previews and logs redact known sensitive fields, but do not share raw-editor screenshots or assume arbitrary unknown extension fields can be automatically classified as secrets. Revision checks prevent a stale native editor or Apply confirmation from overwriting a newer draft. Application failure attempts to restore the last running configuration; it cannot guarantee recovery from arbitrary host/network failures.

Connections shows core observations, not predicted rule matches. `h` includes recent closed connections, `x` closes one, and `r` refreshes. The manager limits displayed samples to 500 records; this is not complete traffic history. Logs and local Diagnostics are separate from the explicit `v` HTTPS connectivity probe. Browser slowness has not been proven fixed by this release.

Runtime files remain in the user's private data directory. Existing `sbtui` directories are still found to avoid orphaning a running manager. `--data-dir` selects an isolated instance. Source Git excludes runtime files, logs, credentials and builds.

## Verification

```sh
cargo test --offline --locked
cargo fmt --check
cargo clippy --offline --all-targets -- -D warnings

SING_TEST_CORE=/absolute/path/to/sing-box cargo test --offline -- \
  --ignored --skip install_official_core --skip public_youtube_rule_conversion --nocapture
```

The explicit suite uses temporary fictional data, loopback servers and a read-only macOS inventory check. System-proxy writes are tested with a MOCK helper. No user subscription, real system-proxy write, TUN activation or public DNS/bandwidth test is included. Tests cover migration backup and redaction, native round trips, stale edits/reviews, resolver template validation, nested selectors, draft-vs-running state, apply/rollback, cross-format imports and existing lifecycle behavior.

Remaining work includes full platform acceptance, richer native-field forms, native full-profile import assistance, DNS/path timing diagnostics, subscription scheduling, automatic membership policies and hardened service/autostart integration. The architecture keeps native capabilities accessible without claiming all these workflows are finished.
