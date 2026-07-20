# Security & Code Review — 2026-07-20

**Scope:** Dependency security audit + Dependabot enablement + code review.
**Branch:** `chore/security-and-review`
**Outcome:** 10 `cargo audit` vulnerabilities + 1 additional GitHub Dependabot alert resolved; `openssl`/`native-tls` eliminated from the dependency tree entirely; 0 regressions.

---

## 1. Security audit

### Before

`cargo audit` reported **10 vulnerabilities** (7 advisories, 3 duplicated across versions) plus **5 unsound/yanked warnings**:

| Crate | Version | Advisory | Severity |
|---|---|---|---|
| `quick-xml` | 0.36.2 | RUSTSEC-2026-0194 (quadratic run time, duplicate attrs) | high (7.5) |
| `quick-xml` | 0.36.2 | RUSTSEC-2026-0195 (unbounded namespace allocation, DoS) | high (7.5) |
| `tokio-postgres` | 0.7.17 | RUSTSEC-2026-0178 (panic on short `DataRow`, DoS) | medium (6.9) |
| `postgres-protocol` | 0.6.11 | RUSTSEC-2026-0179 (unbounded SCRAM iterations, DoS) | high (8.7) |
| `postgres-protocol` | 0.6.11 | RUSTSEC-2026-0180 (panic decoding malformed `hstore`) | medium (6.9) |
| `quinn-proto` | 0.11.14 | RUSTSEC-2026-0185 (unbounded stream reassembly, DoS) | high (7.5) |
| `rustls-webpki` | 0.103.10 | RUSTSEC-2026-0104 (reachable panic in CRL parsing) | n/a |
| `rustls-webpki` | 0.103.10 | RUSTSEC-2026-0098 (URI name constraints incorrectly accepted) | n/a |
| `rustls-webpki` | 0.103.10 | RUSTSEC-2026-0099 (wildcard name constraints incorrectly accepted) | n/a |
| `crossbeam-epoch` | 0.9.18 | RUSTSEC-2026-0204 (invalid pointer deref in `fmt::Pointer`) | n/a |
| `anyhow` | 1.0.102 | RUSTSEC-2026-0190 (unsound `Error::downcast_mut`) | unsound |
| `rand` | 0.8.5 / 0.9.2 / 0.10.0 | RUSTSEC-2026-0097 (unsound with custom logger) | unsound |
| `spin` | 0.9.8 | yanked | yanked |

### After

Direct bumps in `Cargo.toml` / `rswarm_examples/Cargo.toml`:

| Crate | From | To |
|---|---|---|
| `quick-xml` | `0.36.2` | `0.41` |
| `tokio-postgres` | `0.7.13` | `0.7.18` |
| `anyhow` | `1.0.89` | `1.0.103` |

(`rustls` stayed at `"0.23"` — its semver range already admitted the fixed `0.23.37`; `cargo update` pulled the patched transitive deps.)

Resolved in the lockfile:

| Crate | Resolved version |
|---|---|
| `quick-xml` | 0.41.0 |
| `tokio-postgres` | 0.7.18 |
| `postgres-protocol` | 0.6.12 |
| `anyhow` | 1.0.104 |
| `quinn-proto` | 0.11.16 |
| `rustls-webpki` | 0.103.13 |
| `crossbeam-epoch` | 0.9.20 |

### Verification

```
cargo update                                   # lockfile refreshed
cargo build --all-targets --all-features       # exit 0, 0 warnings
cargo test --workspace --all-features          # 169 passed, 0 failed
cargo clippy --workspace --all-targets --all-features -- -D warnings  # exit 0
cargo fmt --all --check                        # exit 0
cargo audit                                    # 0 vulnerabilities, 0 warnings
cargo tree -i openssl --all-features           # "did not match any packages"
cargo tree -i native-tls --all-features        # "did not match any packages"
```

### Notes on the `quick-xml` 0.36 → 0.41 jump

This was the only bump spanning a major-API boundary. Verified safe because rswarm's entire usage is three lines: `quick_xml::de::from_str` (signature stable), `quick_xml::DeError` (still re-exported), and `#[serde(rename = "@attr")]` syntax for XML attributes (unchanged). `Steps`/`Step` deserializers in `src/types.rs:1859-1891` compiled unchanged.

### Remaining advisories

**None.** All 10 `cargo audit` vulnerabilities and all 5 unsound/yanked warnings cleared. The three `rand` RUSTSEC-2026-0097 entries (one per resolved version) were all transitively pulled by now-updated parents (`quinn-proto`, `tokio-postgres`, `headless_chrome`).

### Reconciliation with GitHub Dependabot alerts (17 on `main`)

GitHub's Dependabot reported **17 alerts on `main`** — a broader set than `cargo audit`'s 10 because GitHub scans the default branch continuously and includes alerts fixed by transitive updates it hadn't yet rescanned. Cross-referencing each of the 17 alerts against this branch's resolved versions:

| Status | Count | Packages |
|---|---|---|
| Already fixed on this branch (stale alerts — auto-close on merge) | 16 | `rand` (×3), `rustls-webpki` (×3), `actix-http`, `openssl` (×7), `cmov` |
| Genuinely still vulnerable — **fixed in this PR** | 1 | `opentelemetry_sdk` 0.31.0 → 0.32.1 (GHSA-w9wp-h8wv-79jx / CVE-2026-48504) |

### `opentelemetry_sdk` 0.31 → 0.32 (the 1 remaining real alert)

Bumped the full otel stack together (`opentelemetry`, `opentelemetry_sdk`, `opentelemetry-otlp` 0.31 → 0.32; `tracing-opentelemetry` 0.32 → 0.33, which tracks the otel API version). Despite 0.32 carrying breaking changes for some consumers, rswarm's entire otel footprint is one function — `init_tracer` in `src/observability.rs` (~50 lines, behind the `otel` feature flag, no tests) — and its API surface (`SdkTracerProvider::builder()`, `Resource::builder()`, `SpanExporter::builder().with_tonic()`, `with_batch_exporter`) is stable across the bump. **Zero source changes required.**

### `openssl` eliminated from the dependency tree (architectural fix)

7 of the 17 GitHub alerts were against `openssl`, which was in the tree as dead weight: rswarm requests `rustls-tls` but never set `default-features = false` on its `reqwest` dependency, so Cargo's feature unification also pulled the *default* `default-tls` → `native-tls` → `openssl` stack. The codebase never touches openssl. Additionally, `mcp_rs` (a dev-dependency with **zero references** anywhere in `src/`, tests, or examples — an orphan from commit `60df4cb`) was the sole remaining puller of `native-tls` via its own `reqwest` + `reqwest-eventsource` deps.

Fixes applied:
- `Cargo.toml` / `rswarm_examples/Cargo.toml`: `reqwest default-features = false` (keeps `json`/`stream`/`rustls-tls`; drops `default-tls`, `charset`, `http2`, `system-proxy`, none of which the codebase uses).
- `Cargo.toml`: removed the `mcp_rs` dev-dependency. `actix-web` dev-dep retained (actively used by `src/tests/swarm_run.rs` mock servers).

Result: `cargo tree -i openssl` and `cargo tree -i native-tls` both return *"did not match any packages"*. The lockfile shrank by ~800 lines. The 7 openssl advisories can no longer recur in any future version.

---

## 2. Dependabot & CI

- **Added `.github/dependabot.yml`** — weekly cadence (Monday), `cargo` ecosystem at repo root (covers both workspace members), patch+minor updates grouped into a single PR, majors kept separate for individual review. Also tracks `github-actions` ecosystem.
- **Added `cargo audit` job** to `.github/workflows/ci.yml` via `rustsec/audit-check@v2.0.0`. Future advisories will now fail CI at PR time — closing the gap that left these 10 vulnerabilities undetected for months.

### Manual follow-up (repo owner)

After merging this PR, GitHub will rescan `main` and the 16 stale Dependabot alerts will auto-close. To monitor alerts via the CLI going forward, the `gh` token can be broadened with `gh auth refresh -h github.com -s security_events` (the `repo` scope alone is sometimes sufficient for reads, as observed during this audit).

---

## 3. Code review findings

**Summary: no safe inline fixes were required.** `cargo clippy -D warnings` passed clean and no warnings were introduced by the version bumps. The findings below are architectural observations for future work, **not actioned in this PR**.

### 3.1 Panic-surface audit — *healthy, no action needed*

A full pass over `.unwrap()` / `.expect()` in non-test source (excluding `src/tests/**` and inline `#[cfg(test)] mod tests`) found:

- **0 production `.unwrap()` calls.** The headline `327` count reported by `grep` is misleading — ~93% live inside test modules. Every external-facing failure mode (Mutex/RwLock locks, HTTP, JSON parsing, env reads, DB, channels) uses `Result` propagation via `?` / `.map_err(...)?`.
- **13 production `.expect()` calls**, all on compile-time constants and labeled with `SAFETY:` comments:
  - 10 in `src/types.rs` `impl Default` blocks — construct validated newtypes from named constants (`DEFAULT_REQUEST_TIMEOUT`, `DEFAULT_MAX_LOOP_ITERATIONS`, etc.). Each has a paired regression test.
  - 2 in `src/guardrails.rs` — `Regex::new(...).expect(...)` inside `OnceLock` caches for static injection/PII pattern literals.
  - 1 in `src/util.rs:149` — static `<steps>` extraction regex.

**Optional hardening (low priority):** add `clippy::unwrap_used` / `clippy::expect_used` to `clippy.toml` or workspace lints with an allow-list override for the 13 documented sites, to keep the invariant as the codebase grows.

### 3.2 Large-file structure — *candidate for future module splits*

| File | Lines | Notes |
|---|---|---|
| `src/core.rs` | 3,051 | `impl Swarm` alone is ~2,400 lines; contains 8 cohesive feature blocks |
| `src/types.rs` | 2,053 | 9 distinct clusters: newtypes, agent domain, config, OpenAI wire types, response DTOs, step DSL, retry/timeout |
| `src/persistence/sqlite.rs` | 1,217 | Already partially structured via trait-impl blocks; retention subsystem is a clean ~270-line feature |

`core.rs` has clear natural seams (builder vs runtime, inter-agent messaging, team formation, guardrails, persistence hooks, LLM completion, tool dispatch, execution engine, checkpoint I/O). `types.rs` has 9 separable clusters. The existing `src/persistence/` directory-module pattern (`persistence.rs` as mod + `persistence/{sqlite,postgres}.rs` children) is a proven in-repo precedent both files could follow.

**This is a refactor, not a correctness issue.** Defer until a feature change actually touches one of these seams.

### 3.3 Repo housekeeping performed

- Deleted 4 stale local branches from already-merged PRs (`ci/basic-rust-ci`, `cleanup/remove-stub-vector-features`, `refactor/deprecate-agent-functions`, `refactor/run-options-struct`).
- Confirmed no open issues (0 open, 0 closed ever) and no open PRs. The two historical Dependabot PRs (#1 rustls, #2 openssl) were auto-closed by Dependabot as "up-to-date" — no action.
- Root-level stray files (`cargo`, `cargo_err_output`, `echo`, `head`, `output.txt`) are local shell-redirection accidents, already covered by `.gitignore`, and not committed.
