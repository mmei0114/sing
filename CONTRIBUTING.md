# Contributing to sing

Thanks for helping make native sing-box configuration easier to use. Small, focused changes and reproducible reports are welcome. English and Chinese issues are both welcome; the TUI itself is English-only.

## Report a problem

Use the [issue templates](https://github.com/mmei0114/sing/issues/new/choose). Include your OS/architecture, terminal size, `sing --version`, core version, capture mode, steps and expected/actual behavior. Say whether you were using a local terminal or SSH.

Reproduce with fictional data or `sing --demo` when possible. Never attach subscription URLs, tokens, raw `state.json`/`runtime.json`, private backups or unredacted native-editor screenshots. Read [Security](SECURITY.md) for sensitive reports. A latency measurement is not a throughput benchmark.

## Build and check

Install Rust/Cargo (tested: 1.94.0), Git and your platform's C linker/build tools.

```sh
cargo build --locked
cargo test --locked
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo run --locked -- --demo
```

After dependencies are cached, `--offline` can be added. Ordinary tests do not need a core. CI runs ordinary tests and build checks on macOS and Linux; a green job is not evidence of privileged takeover or actual SSH networking.

Explicit local/core tests are ignored by default. On a suitable host, with a compatible sing-box 1.14.0 executable:

```sh
SING_TEST_CORE=/absolute/path/to/sing-box cargo test --locked -- \
  --ignored --skip install_official_core --skip public_youtube_rule_conversion --nocapture
```

This selection uses temporary fictional state, loopback sockets, a mock system-proxy helper and read-only macOS inventory. The skipped tests access public download endpoints. Do not run arbitrary takeover tests against your normal data directory or a critical SSH connection.

## What to preserve

- One authoritative native document. Forms change their fields without deleting unknown siblings.
- Save is a draft operation; Apply is explicit. Browsing or importing must not take over the host network.
- Node subscriptions and rule sets are different resources. Rules and DNS are independently configured.
- Unsupported conversions are reported. Do not silently broaden matching conditions.
- Runtime claims come from API observations, not a successful button press or a cached selection.
- Keep primary tasks visible and keyboard-accessible at 80×24. Test long Unicode names and cancellation/error paths.

See the [product specification](docs/product-spec-v1.md) and [verification record](docs/acceptance-0.6.0.md). Discuss changes to navigation, default networking or configuration semantics before a large PR.

## Submit a pull request

1. Fork the repository and create a focused branch.
2. Add a regression test for a bug or fixtures for a new rule format. Use `.invalid`/`.test` domains and fictional credentials.
3. Run the checks above and describe exactly what you tested; name untested platforms.
4. Include a demo-data screenshot for UI changes and update affected English/Chinese documentation.

To regenerate the README image after a release build, run `node scripts/render-preview.mjs` (Node.js 18+). This renders the real `--preview` output, not a mock UI. `node scripts/check-docs.mjs` checks local documentation links.

Do not include `.build/`, `target/`, logs, real network traces or private application data. Contributions to sing are under the project's [MIT license](LICENSE); do not copy incompatible third-party implementation code into the client.
