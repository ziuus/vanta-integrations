# Vanta Integration Development — Engineering Handoff

Live working document for the `vanta-integrations` ecosystem. Update it before
ending any substantial work session. The repository and the running
implementation are the source of truth; this file records **decisions,
constraints and evidence** that are expensive to rediscover.

Companion repo: `../vanta` (the host). Host version at time of audit: **0.10.25**.

---

## 1. Architecture Findings (Phase 0 audit — evidence-based)

### 1.1 Two unrelated extensibility mechanisms exist

| Mechanism | Lives in | Data access | Distribution |
|---|---|---|---|
| **WASM extensions** (this repo) | `~/.config/vanta/extensions/*.wasm` | **None** (see 1.3) | `vanta ext install` from `registry.json` |
| **Custom widgets** (host feature) | `[[custom_widgets]]` in `config.toml` | Full: shell command + file reads, host-side | Not distributable; user config only |

`src/custom/` in the host runs user-defined commands/file reads on the host and
renders the result. It is *not* the extension system and is not installable.
Do not confuse the two. Everything in this repo is the WASM path.

### 1.2 WASM ABI (exact, from `vanta/src/extension/wasm.rs`)

A plugin is a `cdylib` for `wasm32-wasip1` exporting exactly three functions
via `extism-pdk`:

```rust
#[plugin_fn] pub fn metadata() -> FnResult<Vec<u8>>       // JSON ExtensionMetadata
#[plugin_fn] pub fn widgets() -> FnResult<Vec<u8>>        // JSON Vec<String> of widget ids
#[plugin_fn] pub fn render_widget(id: String) -> FnResult<Vec<u8>>  // JSON UiWidget
```

`ExtensionMetadata` = `{id, name, author, version, api_version, description}`.

Host load path (`vanta/src/main.rs`): scans `~/.config/vanta/extensions/`,
constructs `WasmExtension::new(path)`, registers it **only if** the extension id
appears in `config.toml` `[extensions] enabled = [...]`
(`ExtensionManager::register`). Widgets are then placed by id in
`[dashboard] layout`, matched case-insensitively against either the **widget id**
or the **extension id** (`screens/dashboard.rs`).

Hard constraints baked into the host:

- **API version gate**: `metadata.api_version` must start with `"0.9"` —
  enforced in *both* `extension/wasm.rs:32` and `cli.rs:159`. A plugin
  declaring `0.10.x` is rejected at load **and** at install.
- **10 ms execution timeout per call** (`Manifest::with_timeout`). This is the
  budget for `render_widget`, *per widget, per frame*. Exceeding it kills the call.
- **1 MB max payload**, **depth ≤ 16**, **≤ 2000 spans**, **≤ 100 000 chars**
  (`protocol.rs`). Violations are dropped silently — the widget renders blank.
- Errors from `render_widget` are swallowed (`Err(_e) => {}`). A failing plugin
  shows an empty area, never a crash. Debug with `examples/probe_host.rs`.
- `render_widget` is called **on the render thread, every frame**. There is no
  `tick`/`update` entry point and no background execution for WASM plugins.
- WASM components **cannot receive key events**. `Component::handle_key` exists
  in the native trait but `WasmComponent` does not implement it. Extensions are
  display-only today.

### 1.3 Host capability probe — what a plugin can actually reach

Measured (not assumed) by loading a probe plugin through the host's own loader
(`vanta/examples/probe_host.rs`, kept in the host repo for re-verification):

```
FS /proc/stat      ERR No such file or directory (os error 44)
FS readdir /proc   ERR No such file or directory (os error 44)
ENV HOME           Err(NotPresent)
TIME               Ok(1789875833)      <- wall clock works
extism config      Ok(None)            <- host passes no config
extism vars        Ok(None)            <- host sets no vars
```

Cause: `Plugin::new(&manifest, [], true)` — the `[]` is the **host-function
list (empty)** and the manifest declares no `allowed_paths` and no
`allowed_hosts`. WASI is enabled but nothing is preopened.

**Therefore, as of host 0.10.25, a WASM extension can only use:**
- pure computation,
- the wall clock,
- whatever it hard-codes.

No filesystem. No network. No process list. No host telemetry. No config.

### 1.4 Consequence for the roadmap (the central finding)

The existing integrations are honest about this:
- `security` renders a **hard-coded mock** CVE list (`security/src/lib.rs`).
- `crypto_coin` is **pure math** (a raymarched coin) — no data at all.

Every extension in the requested roadmap (`network_command`,
`process_explorer`, `container_fleet`, `system_observatory`,
`developer_workspace`, `event_stream`, `resource_timeline`) is **entirely
telemetry-driven**. With the current ABI they could only be built as fake
dashboards, which the project rules explicitly forbid ("Never display fake
telemetry as live system data").

**The blocker is in the host, not in this repo.** Building extensions first
would mean building mockups.

### 1.5 Resolution: host telemetry host-function (decided)

The host *already samples* everything required, on its own sampler thread:
`monitors::summary()`, `cpu::snapshot()`, `memory::snapshot()`,
`network::snapshot()`, `disk::mounts()`, `gpu::snapshot()`,
`processes::top_by_cpu()/count()`. Exposing that existing data to plugins is:

- the standard Extism mechanism (host functions — the empty `[]` slot),
- zero additional sampling cost (no duplicated collection),
- consistent with the documented architecture rather than a replacement for it.

Plan: add one host function to Vanta

```
vanta_query(request_json) -> response_json
```

so extensions read **real, already-collected** telemetry. Anything the host
does not collect stays unavailable and must be reported as such by the
extension (never invented).

Versioning constraint discovered above: the gate accepts only `0.9*`, so the
capability is introduced **without** bumping plugin `api_version` past `0.9.x`
(plugins keep declaring `0.9.x`; capability is discovered at runtime by calling
`vanta_query` and handling absence). This keeps existing `security` and
`crypto_coin` artifacts loading unchanged on new hosts, and new plugins
degrade gracefully on old hosts.

### 1.6 UI protocol (`vanta/src/protocol.rs`) — the whole vocabulary

```
UiWidget = Paragraph { lines, block, wrap }
         | Gauge     { ratio, label, block, color }
         | List      { items, block }
         | Column    { children, percentages }   // vertical split
         | Row       { children, percentages }   // horizontal split
```
`UiLine { spans: [UiSpan { content, style }] }`,
`UiStyle { fg, bg, bold, italic, underlined }`,
`UiColor` = named | `Rgb(r,g,b)` | `"#rrggbb"` (untagged string).
`UiBlock { title, bordered, border_color }`.

There is **no** sparkline, chart, table or canvas primitive. Every graph,
table and gauge beyond the basic `Gauge` must be composed from styled text
spans by the plugin. This is where shared visual primitives (section 15 of the
brief) genuinely earn their place — as a plugin-side SDK crate, not as new host
widgets.

`percentages` on Row/Column is `Option<Vec<u16>>`; when `None` the host splits
evenly. Layout is the only geometry available — a plugin **cannot query its own
area size**, so widgets must be written to look correct at any size. This is a
real constraint for the 80×24 requirement: prefer content that degrades by
wrapping/truncating rather than art that assumes a width.

### 1.7 Registry / distribution (`vanta/src/cli.rs`)

`registry.json` at repo root, fetched raw from GitHub `main`:
```json
{ "api_version": "0.9.1",
  "extensions": [ { "id","name","description","version","api_version",
                    "author","wasm_url","sha256" } ] }
```
`vanta ext install <id>` downloads `wasm_url`, verifies **sha256** (hard fail on
mismatch), rejects non-`0.9*` `api_version`, writes to
`~/.config/vanta/extensions/<id>.wasm`. `vanta ext search`, `vanta ext update`
also exist. Artifacts are committed to `artifacts/` in this repo and served via
`raw.githubusercontent.com`.

Rule: **build the artifact, then record its real sha256**. Verified current
entries match their artifacts (`sha256sum artifacts/*.wasm`).

### 1.8 Build & validation commands (verified working)

```bash
# in vanta-integrations
cargo build --target wasm32-wasip1 --release     # target already installed
sha256sum target/wasm32-wasip1/release/<name>.wasm

# headless plugin inspection (in ../vanta) — NOTE: debug profile;
# `--release` OOMs the machine because of lto="thin" + codegen-units=1
cargo run --example probe_host -- /path/to/plugin.wasm

# host validation (in ../vanta)
cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test
```

Note the artifact name: `security/Cargo.toml` has `name = "vanta-security"`, so
it builds `vanta_security.wasm` but is published as `artifacts/security.wasm`.
The **file stem does not have to match the extension id** — the host reads the
id from `metadata()`, not the filename.

### 1.9 Findings from building the first real extension (M2/M3)

* **Widget ids share one global namespace.** The dashboard dispatcher matches
  a layout entry against *either* a widget id or an extension id
  (`screens/dashboard.rs`), across all loaded extensions. Two extensions
  exposing the same widget id would collide, first match winning. This is why
  `resource_timeline` ships as a **widget of `system_observatory`** rather than
  the separate extension of roadmap §12 — a standalone one would clash.
* **Border style is host-controlled.** Plugins set `bordered: true`; the host
  picks the glyphs. It used to draw square corners while native panels use
  `BorderType::Rounded`, making every extension look foreign. Fixed in the
  host (`src/ui_renderer.rs`), not per plugin.
* **Query once per frame, not once per widget.** The host calls
  `render_widget` separately for each widget, so five widgets each querying
  six topics is 30 host round-trips per frame. A short-TTL cache
  (`system_observatory/src/state.rs`) fixes both the cost and the correctness
  problem of panels disagreeing within one frame.
* **Measured render cost** (debug host, release plugin): first call of a frame
  ~7 ms (cold: six queries + capabilities), cached calls 0.2–2.9 ms. The
  budget is 10 ms per call, so the cold path has less headroom than it looks —
  do not add more topics to a single refresh without measuring.
* **State persists across calls.** `thread_local!` + `RefCell` works and is
  preferable to `static mut` (no `static_mut_refs` lint, same cost). WASM
  plugins are single-threaded.
* **`wrap: true` is needed for any line that can exceed a narrow cell.**
  With `wrap: false` the host clips, silently hiding content.
* **A plugin cannot know its area**, so graph widths are fixed constants
  chosen for a 3-column 80-wide dashboard (18 cells) and full-width rows
  (40 cells); wider terminals simply leave space on the right.

---

## 2. Current State

| Component | Status | Data | Notes |
|---|---|---|---|
| host `vanta_query` | **shipped** (vanta `92d5fbc`) | real | 9 topics + capabilities; 88 host tests green |
| host rounded borders | **shipped** (vanta `41482e5`) | — | extension panels now match native chrome |
| `vanta-ext-sdk` | **shipped** (`8eff279`) | — | telemetry client, bounded history, UI primitives; 18 tests |
| `system_observatory` | **shipped** (`96698a1`) | **real** | 5 widgets; 8 tests; artifact + registry published |
| `security` | shipped, v0.1.0 | **mock** | 1 widget `cve_feed`; unchanged, still hard-coded |
| `crypto_coin` | shipped, v1.1.0 | none (pure math) | 1 widget `coin`; unchanged |
| rest of roadmap | not started | — | see §4 |

Nothing in this repo is broken. All three artifacts build and every registry
sha256 was verified against its committed artifact.

## 3. Known Limitations (host, as of 0.10.25)

- No filesystem, network, env, or host config inside plugins (measured, 1.3).
- No key input to WASM widgets → no interactive tables/filtering/sorting.
- No self-size query → layouts must be size-agnostic.
- No background work / no tick → all computation happens inside a 10 ms render call.
- No persistent state across calls except WASM linear memory (`static mut`),
  which *does* persist between calls within a process (used by `crypto_coin`
  for its animation tick).
- Container/Docker, git state, systemd journal, connection tables,
  per-interface network: **not collected by the host at all** — even with the
  telemetry host function these stay unavailable until the host collects them.
  `{"topic":"capabilities"}` returns this list at runtime.
- `cargo clippy --workspace -- -D warnings` currently **fails on 24
  pre-existing lints in `crypto_coin`** (`manual_range_contains` etc., from
  commit `fdc5f94`). New crates are clean. These were deliberately left alone:
  fixing them changes a published artifact's source without rebuilding it,
  which would put source and the registry sha256 out of step. Fix and
  republish the artifact together, in one commit.

## 4. Remaining Work

- [x] **M1** Host `vanta_query` telemetry API + docs.
- [x] **M2** `vanta-ext-sdk` — telemetry client, history, UI primitives.
- [x] **M3** `system_observatory` — first real-telemetry extension, proven
      end-to-end (build → artifact → registry → live render in Vanta).
- [ ] **M4** `process_explorer`. Host already serves pid/ppid/cpu/rss/state/
      threads/uid/io, so `process_tree`, `process_cpu`, `process_memory`,
      `process_io`, `process_top` are all buildable today. Note: WASM widgets
      receive **no key events**, so sorting/filtering cannot be interactive —
      expose sort order via `[extensions.process_explorer]` config instead...
      except config is not passed to plugins either (§1.3), so the first
      version must ship fixed, sensible orderings per widget.
- [ ] **M5** `network_command`. Only aggregate rx/tx exists. `interface_status`,
      `connection_table` and `network_topology` need host collectors that do
      not exist — either add them to the host first or ship the extension with
      explicit unavailable panels. Do **not** fabricate interfaces.
- [ ] **M6** `event_stream`. No host event source exists. Designing it means
      first deciding where events come from (host collector vs. derived from
      telemetry transitions in the plugin). Deriving from telemetry deltas is
      honest and needs no host change — prefer that for v1.
- [ ] **M7** `container_fleet`, `developer_workspace` — both require new host
      collectors (container runtime, git). Scope only after M4/M5.
- [ ] Improve `security` (still mock; needs a real CVE source, which needs
      host network access — currently impossible) and `crypto_coin` (market
      data likewise needs network).
- [ ] Optional: fix 24 pre-existing clippy lints in `crypto_coin` (see §5).

---

## Resume Point

**Last completed:**
M3 — `system_observatory`, the first extension running on real host telemetry,
proven end-to-end: built for `wasm32-wasip1`, copied into
`~/.config/vanta/extensions/`, enabled in `config.toml`, and rendered in a
live Vanta session with its numbers matching the native panels (load
`3.33 3.84 3.24`, disk 88%). Artifact committed and registry entry carries its
real sha256. Also M1 (host `vanta_query`) and M2 (`vanta-ext-sdk`).

Commits — host (`../vanta`): `92d5fbc` telemetry API, `41482e5` rounded
borders. Integrations: `de3e716` audit, `8eff279` sdk, `96698a1`
system_observatory.

**Currently working on:**
Nothing in flight. The tree is clean and validated in both repos.

**Next action:**
M4 `process_explorer`. All required data already exists in the
`{"topic":"processes","limit":N}` response (pid, ppid, name, cmdline, cpu_pct,
mem_kb, state, threads, uid, read_bps, write_bps) — no host change needed.
Suggested widgets: `process_explorer` (overview + top table), `process_tree`
(build the hierarchy from ppid; note the host returns only the top N by CPU,
so parents may be missing — render orphans at root rather than dropping them),
`process_cpu`, `process_memory`, `process_io`, `process_activity`.
Copy the shape of `system_observatory`: a `state.rs` with a short-TTL shared
snapshot, pure logic in its own module with unit tests, widgets in `lib.rs`.
Reuse `vanta_ext_sdk::ui::Table` — it already pads and clips per column.

**Files to create:**
- `process_explorer/{Cargo.toml,README.md}`
- `process_explorer/src/{lib.rs,state.rs,tree.rs}`
- add to workspace `members`, then artifact + `registry.json` entry.

**Known issues:**
- `cargo clippy --workspace -- -D warnings` fails on **24 pre-existing lints in
  `crypto_coin`** only. Validate new crates with `-p <crate>`; see §3 for why
  they were not fixed in isolation.
- Host `cargo run --release --example ...` OOMs (LTO). Use the debug profile.
- `rustup target add wasm32-wasip1` reports a component conflict; the target is
  installed and builds fine. Ignore.

**Validation status (all re-run at the end of this session):**
- `cargo test -p vanta-ext-sdk` — 18 passed
- `cargo test -p system-observatory` — 8 passed
- `cargo clippy -p vanta-ext-sdk -p system-observatory --all-targets -D warnings` — clean
- `cargo fmt --all -- --check` — clean
- `cargo build --target wasm32-wasip1 --release` — all crates build
- registry sha256 verified against all three committed artifacts
- host `cargo test` — 88 passed; host builds in release
- live render in Vanta confirmed on the Dashboard page

**Do not redo:**
- The capability probe (§1.3). Definitive: no FS, no network, no env, no
  config inside plugins. Telemetry comes only from `vanta_query`.
- The host telemetry API. It works; add topics to it rather than replacing it.
- The shared-snapshot pattern and SDK primitives (sparkline, braille, table,
  meters, bounded history) — reuse them, do not reimplement per extension.
- Do not fix `crypto_coin` lints without rebuilding and republishing its
  artifact and sha256 in the same commit.
- Do not build roadmap extensions on mock data.

**User's environment left untouched:**
`~/.config/vanta/config.toml` was temporarily modified for the live test and
**restored** from `/tmp/vanta-cfg.bak`; `enabled = ["crypto_coin"]` and the
original layout are back. `system_observatory.wasm` remains installed in
`~/.config/vanta/extensions/` but is not enabled, so it does not load.
