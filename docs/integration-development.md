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

---

## 2. Current State

| Extension | Status | Data | Notes |
|---|---|---|---|
| `security` | shipped, v0.1.0 | **mock** | 1 widget `cve_feed`; hard-coded CVEs |
| `crypto_coin` | shipped, v1.1.0 | none (pure math) | 1 widget `coin`; 3D raymarched |
| everything in roadmap | **not started** | blocked on 1.4/1.5 | needs host telemetry |

Nothing in this repo is broken. Both artifacts build and their registry hashes
are correct.

## 3. Known Limitations (host, as of 0.10.25)

- No filesystem, network, env, or host config inside plugins (measured, 1.3).
- No key input to WASM widgets → no interactive tables/filtering/sorting.
- No self-size query → layouts must be size-agnostic.
- No background work / no tick → all computation happens inside a 10 ms render call.
- No persistent state across calls except WASM linear memory (`static mut`),
  which *does* persist between calls within a process (used by `crypto_coin`
  for its animation tick).
- Container/Docker, git state, systemd journal, connection tables: **not
  collected by the host at all** — even after the telemetry host function
  these remain unavailable until the host collects them.

## 4. Remaining Work

- [ ] **M1** Host: `vanta_query` host function exposing existing telemetry + docs.
- [ ] **M2** SDK crate (`vanta-ext-sdk`) — typed query wrappers + UI builders
      (sparkline, bar, table, metric rows) composed from spans.
- [ ] **M3** `system_observatory` — first real-telemetry extension, end-to-end
      proof (build → artifact → registry → `vanta ext install` → renders).
- [ ] **M4** `process_explorer` (host already has process snapshots).
- [ ] **M5** `network_command` (host has interface rx/tx; connection table is
      *not* collected → document as unavailable, do not fake).
- [ ] **M6** `resource_timeline` / `event_stream` (bounded rolling buffers in
      plugin memory; note the no-tick constraint — history advances per render).
- [ ] **M7** `developer_workspace`, `container_fleet` — require new host
      collectors (git, container runtime). Scope only after M1–M5.
- [ ] Improve `security` (replace mock with real data once a source exists) and
      `crypto_coin` (compact market widgets need network → blocked).

---

## Resume Point

**Last completed:**
Phase 0 architecture audit, with empirical capability probe. Findings recorded
in sections 1.1–1.8 above. `vanta/examples/probe_host.rs` added to the host repo
as a reusable headless plugin inspector.

**Currently working on:**
M1 — adding the `vanta_query` host function to the host repo so extensions can
read telemetry Vanta already samples.

**Next action:**
In `../vanta`: add host function(s) to `Plugin::new(&manifest, [], true)` in
`src/extension/wasm.rs`, serving a JSON request/response backed by
`monitors::{summary, cpu, memory, network, disk, gpu, processes}`. Keep the
`0.9*` api_version gate intact (see 1.5). Then document the capability in
`vanta/docs/EXTENSIONS.md` and re-run the probe to prove it end-to-end.

**Files being modified:**
- `../vanta/src/extension/wasm.rs` (host functions)
- `../vanta/docs/EXTENSIONS.md` (capability documentation)
- `../vanta/examples/probe_host.rs` (verification harness, already added)
- this document

**Known issues:**
- `cargo run --release --example ...` in the host OOMs (LTO). Use debug.
- `rustup target add wasm32-wasip1` errors with a component conflict but the
  target **is** installed and builds fine — ignore that error.

**Validation status:**
- `vanta-integrations`: `cargo build --target wasm32-wasip1 --release` passes;
  both artifacts build; registry sha256 values verified against artifacts.
- host: `cargo test` 73 passing, clippy clean (pre-existing state, untouched).

**Do not redo:**
- The capability probe. The answer is definitive: no FS, no net, no env, no
  config inside plugins (1.3). Do not re-litigate by "trying `std::fs` again".
- Do not attempt to build roadmap extensions on mock data — that is the exact
  failure mode this audit exists to prevent.
- Do not bump plugin `api_version` above `0.9.x` until the gate in **both**
  `wasm.rs` and `cli.rs` is widened.
