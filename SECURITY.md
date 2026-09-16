# Security and privacy

sing manages network configuration and may request elevated permission for System Proxy or TUN. It is an early-stage client, not an audited security boundary. The currently maintained release line is 0.6.x; no response-time guarantee is offered.

## Report privately

Use GitHub's [private vulnerability reporting](https://github.com/mmei0114/sing/security/advisories/new) for credential exposure, unsafe privilege handling, unexpected network takeover or similar security issues. If private reporting is unavailable, open an issue asking for a private contact **without disclosing the vulnerability or any secret**.

Include a minimal fictional reproduction, version, platform and impact. Do not include real credentials even in the initial private report. Never put an exploit with sensitive data in a public issue.

## Keep these private

- Subscription URLs and their query strings, provider tokens and node passwords/keys.
- Raw native-editor contents, `state.json`, `runtime.json`, `selections.json`, private state backups and captured traffic.
- Screenshots or logs containing server addresses, process paths or browsing history you do not want public.

Normal snapshots/diffs redact known secrets; raw native editors deliberately show configuration. Unknown extension fields cannot be universally recognized as secrets. Review every attachment manually.

## Safe operation

- Try `sing --demo` before real setup. It uses fictional data and does not change networking.
- Back up private state before upgrades. Stop a managed instance before restoring compatible state; a source-code backup does not back up your subscriptions.
- Keep the management API loopback-only. Do not expose its address or secret through a public dashboard.
- TUN and system proxy changes can disrupt traffic. Have a recovery path, especially over SSH. `q` closes only the UI; use Stop or `sing --shutdown` when you intend to stop the manager/core.
- The core installer verifies a pinned upstream archive digest. It does not establish that your proxy provider, rule source or DNS server is trustworthy.

No blanket claim is made about DNS leakage, malicious proxy providers or host takeover recovery. See the [verification limits](docs/acceptance-0.6.0.md).
