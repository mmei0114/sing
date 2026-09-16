# Changelog

## 0.6.0 — first public preview

- Five task-oriented workspaces: Overview, Proxies, Routing, Network and Activity; global Settings and visible keyboard actions.
- Subscription previews, Update All, optional User-Agent and connection setup with explicit draft/save/apply steps.
- One-transaction manual/automatic groups, nested members and searchable member selection.
- Rule import with target/position binding and staged inline group creation; native JSON, Source and SRS paths stay native.
- Shared native references, dependency-aware removal, semantic Review Changes and redacted Native Diff.
- API-confirmed live group selection, separate selection memory and default-aware Apply recovery.
- Error details, preserved input, stale-preview recovery and safer credential redaction.
- Public English/Chinese documentation, MIT license, contribution/security guidance and macOS/Linux CI configuration.
- Linux strict-build fix: compile macOS-only proxy transactions/watchdog only for the supported production platform or portable tests; retain `-D warnings`.

Local verification: 122 ordinary tests and 9 explicit isolated/read-only tests passed with sing-box 1.14.0 on macOS arm64. [Public macOS/Linux CI](https://github.com/mmei0114/sing/actions/runs/35065643935) also passed. Build/test results are separate from real System Proxy/TUN, Linux/SSH networking and first-time user acceptance, which remain incomplete. See the [full record](docs/acceptance-0.6.0.md).

### Upgrading

Manager protocol is now **9**. Close old interfaces, then run `sing --shutdown` (or `./sing --shutdown` from a source checkout) when it is safe to interrupt the connection, and reopen sing. Check restoration errors before continuing. See the [upgrade guide](docs/usage.md#upgrade-safely).

This release does not bundle sing-box. Build sing from source using the README instructions, then install/select the core inside Settings. No official package-manager distribution is claimed.
