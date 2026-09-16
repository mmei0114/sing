# sing 0.6.0 — verification and handoff

Date: 2026-09-16. Build host: macOS / arm64. Test core: sing-box 1.14.0.

This is a runnable release of the product-workspace upgrade, not certification of every platform or completion of the full usability goal. No real subscriptions, active manager, system proxy settings or TUN routes were changed during development.

### Public-source CI addendum

The first public CI exposed Linux-only unused macOS backend code. Commit `02bc6b9` isolates that backend to macOS production builds while retaining portable transaction tests on every platform; strict warnings remain enabled. [GitHub Actions run 35065643935](https://github.com/mmei0114/sing/actions/runs/35065643935) then passed on both `ubuntu-latest` and `macos-latest`: formatting, ordinary tests, Clippy, optimized build, version/preview execution and local documentation links. This adds Linux build/test evidence, **not** Linux/SSH host-network or privileged takeover acceptance. The public release is source-only; no signed/notarized binaries or bundled core are advertised.

## Verified

- 122 ordinary tests passed. The ordinary run reports 5 ignored unit tests and 6 ignored integration tests; those are not counted as ordinary passes.
- 9 explicitly selected isolated/read-only tests passed: connectivity probe behavior, read-only macOS proxy inventory, real core lifecycle/gRPC, 2 manager tests, 2 native-flow tests and 2 rule-flow tests.
- The lifecycle test was additionally rerun after adding authentication-failure assertions: no successful-selection persistence or stale group display when API authentication fails.
- Formatting, whitespace checks and Clippy (`--all-targets -- -D warnings`) passed. Optimized macOS arm64 build and launcher version were checked.
- Isolated real-core evidence includes first subscription → saved connection setup → explicit start, native configuration checks, local HTTP proxy traffic, rule/global/direct routing, native SRS loading, confirmed selection, selection persistence across Apply, explicit default-member precedence, rejected stale Apply and rollback after a failed start.
- Unit/render checks cover 54×18, 80×24 and wide terminals, large Unicode member/reference lists, focus and input isolation, cancellation, atomic parent transactions, error-detail recovery, typed references, native field preservation and redacted semantic/native diffs. The diff regression includes long values, absent versus null and terminal-control escaping.
- Release PTY checks use fictional `--demo` data: five workspaces, visible actions, Review Changes / Native Diff / Back and clean terminal exit. Earlier implementation-stage PTY checks additionally exercised group editing, subscription Advanced, references and connection setup. These are developer checks, not first-time user observations.

Commands use the existing workspace dependency cache (`CARGO_HOME=.build/cargo` as an absolute path). Explicit tests also set `SING_TEST_CORE` to `.build/test-core/bin/sing-box` and skip `install_official_core` and `public_youtube_rule_conversion`. There were no external-provider credentials in fixtures. See README for equivalent commands.

## Product-spec acceptance ledger

“Passed (scoped)” means the stated automated/developer evidence, not unobserved user or platform acceptance. Unchecked items in the specification remain required for the overall goal.

| Item | Result | Evidence / boundary |
|---|---|---|
| A01 | Passed | Five workspaces, unique page mapping, global Settings; navigation tests and PTY. |
| A02 | Passed (scoped) | English product controls; import, grouping, rule binding, review and setup have visible keyboard-focusable actions. User discoverability is A03/A18. |
| A03 | Not tested | No observed first-time user's 30-second discovery test. |
| A04 | Partial | Isolated Proxy Ports setup succeeds; source errors, cancel, revision conflicts and privacy have regression tests. Real System Proxy/TUN first-run authorization and recovery are not tested. |
| A05 | Passed | One group transaction, search/multiselect, nested members, defaults/cycles; core validation and PTY. |
| A06 | Passed (scoped) | Converted/native JSON/native local-or-remote reference binding, staged inline group, cancellation and atomic commit. Real local SRS loaded by core. Public remote-resource downloads remain untested. |
| A07 | Passed (scoped) | Native logical conditions, ordered rules, mode overlays and unchanged DNS covered by native/rule-flow tests. Not a proof of equivalence for every third-party rule dialect. |
| A08 | Passed (scoped) | Visible Subscriptions, batch single-save, conflict rejection, source-scoped refresh and preservation of user objects. Conflicts require editing dependencies or source; no automatic arbitrary merge. |
| A09 | Passed (scoped) | DNS/TUN unique editor mapping, existing/custom inbound preservation and SSH setup choices tested. Actual TUN routing is not tested. |
| A10 | Passed | Typed Route/DNS references, shared rule sets, deletion protection and return navigation. Client-only settings references are not in this native graph. |
| A11 | Passed (scoped) | Unknown-field/compound-rule preservation, all-outbound view and native editing regressions. Core/platform support still determines validity. |
| A12 | Partial | API-confirmed selection, auth failure, no draft mutation, Apply restoration and explicit default precedence passed with real core. Cache-owner priority is unit-tested; full native-cache and host-failure recovery are not end-to-end tested. |
| A13 | Passed (scoped) | Save/Apply/Update separation, restart summary, native diff and actual routing-mode regression. |
| A14 | Passed (scoped) | Failed Apply rollback, stale revision rejection, source errors, preserved inputs and preview re-preparation. Real host takeover recovery remains outside this evidence. |
| A15 | Partial | Multiple-size rendering and large-list/focus regressions; representative 80×24 PTY flows. Not every task has been manually repeated at every size. |
| A16 | Passed (scoped) | Keyboard navigation, text input isolation, mouse-free PTY and synthetic SSH scope/choice tests. No real remote SSH session. |
| A17 | Partial | Local tests/lint/release and isolated core checks passed; public macOS/Linux build/test CI also passed (see addendum). Real Linux/macOS takeover acceptance remains incomplete. |
| A18 | Not tested | User still needs to perform the real tasks without chat guidance. |

## Remaining limitations

- No measured speed improvement, DNS leak check, streaming benchmark or actual-provider success claim. Latency tests are not throughput tests.
- Native remote Source/SRS imports retain references rather than fetching/converting them in sing. Content validity and initial reachability are determined by the core on Check/Apply/Start; remote download/update behavior was not verified against a public provider here.
- References currently cover recognized native namespaces, not client-only Global target settings. If a deleted group was only that target, select a new Global target before applying Global mode; effective-config validation rejects missing targets. Unknown future native fields cannot be universally reference-checked or classified as secrets.
- Native cache ownership prevents sing from restoring its own remembered choice. The core's own cache persistence is not certified by the unit test of that precedence decision.
- Real authorization, System Proxy/TUN failure recovery, Linux package distribution and SSH network behavior need dedicated host-specific acceptance. Linux source build/test CI passed; it does not validate host networking. Switching capture while connected can interrupt networking; do not test casually over a critical SSH session.
- Full-profile import assistance, subscription scheduling, dynamic member policies, DNS/path timing tools and hardened autostart are outside this release. Native JSON retains advanced configuration access; this is not equivalent to a dedicated form for every feature.

## Delivery and rollback material

Run `./sing` from the repository after following the README upgrade steps. The launcher uses `target/release/sing` (0.6.0); manager protocol is 9. Development did not stop the old manager. The user must choose when to shut it down, which interrupts its connection.

Local macOS arm64 executable rebuilt after the public-source portability fix, SHA-256: `cfb36465b557e7263d6d15607a89212daad73b70df8950122e95f69d97d2b07f`. This local artifact is not uploaded as a public binary distribution.

The previous executable is retained at `.build/backups/v0.5.1-f70AYn2u/sing` and still reports 0.5.1. SHA-256: `7604898e50ac68b76cda54fedddd0255df7d5efc24eef11c28bacc19ca0ef00a`. Source baseline: `b476cdb`. Binaries, private state and credentials are not included in Git. A binary backup is not a private-data rollback plan; preserve compatible state separately before attempting a downgrade.
