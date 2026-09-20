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

Superseded by the capability audit in **§6**. Kept short here; §6.5 is the
authoritative roadmap.

- [x] **M1** Host `vanta_query` telemetry API + docs.
- [x] **M2** `vanta-ext-sdk` — telemetry client, history, UI primitives.
- [x] **M3** `system_observatory` built and proven end-to-end — then **found to
      duplicate native Vanta** (§6.4). To be reworked, not shipped.
- [ ] **C0** Rework: strip the four duplicate widgets, keep `health.rs` +
      `state.rs`, drop the registry entry and artifact.
- [ ] **C1** `sentinel` — thresholds, sustained breaches, incident timeline,
      correlation. Buildable today; absorbs `event_stream`.
- [ ] **C2** Host collectors (connections → git → containers → security
      posture → per-interface → exec events), each unlocking one extension.
- [ ] **C3** `security` honesty fix (mock CVEs currently labelled "Live").
- [ ] Optional: 24 pre-existing `crypto_coin` clippy lints (§3).
- [ ] **Rejected, do not build**: `process_explorer`, `resource_timeline`,
      `resource_flow`, `load_history`, `traffic_graph`, `network_radar`,
      `network_topology`, `build_status`, `workspace_stats`, `dev_events`.

---

## 6. Capability-Gap Audit (supersedes the widget-driven roadmap)

The roadmap was originally a **list of widgets**. That produced
`system_observatory`, which on inspection is largely a re-skin of what Vanta
already ships. This section re-derives the roadmap from *capabilities* and
records what is rejected and why, so the mistake is not repeated.

### 6.1 Two rejection tests

Every proposed extension must pass both:

1. **Native test** — "could the user already get this from native Vanta?"
   If yes, reject. A different layout, colour or data path is not a capability.
2. **Custom-widget test** — "could the user already get this from a
   `[[custom_widgets]]` entry?" The host ships a user-facing escape hatch:
   `source = command|file` + `renderer = value|text|gauge|bar|graph`
   (`src/custom/`). Anything that is *one scalar or one blob of text from one
   command* is already possible without an extension. An extension only earns
   its place when it needs **logic**: state over time, cross-metric
   correlation, structure, or a workflow.

### 6.2 What native Vanta already provides (verified in source)

* **Dashboard**: `system` (distro logo + os/host/kernel/uptime/shell/term/cpu/
  gpu/memory/battery), `gauges` (cpu/mem/bat rings), `cpu` (history graph +
  per-core meters), `memory` (RAM+SWAP meters and graph), `storage` (per-mount
  capacity bars), `network` (rx/tx rate graphs, peak, totals), `gpu`,
  `status` (wifi ssid+signal, ip, package count + pending updates, **docker
  running/total**, load 1/5/15 + core count, process count, battery with
  watts/ETA, per-sensor temps), `clock`, `calendar`, `weather`, `media`,
  `visualizer`, top-processes preview.
* **Monitor**: large cpu graph + per-core, memory, per-mount disk **I/O**
  graphs + capacity, network rx/tx, gpu, system facts, and a full process
  table — `pid, name, cpu%, MEM%, RSS, state, user, THR, r/s, w/s, COMMAND`
  with select, `/` search, `s` sort field, `r` reverse, `t` tree + `←→` fold,
  `c` command toggle, `k`/`K` SIGTERM/SIGKILL with two-step confirm, plus a
  detail strip (ppid, full cmdline, io).
* **Aesthetic**: block clock, calendar, matrix rain, 3D donut, visualizer,
  pinned media image.
* **Workspace**: agenda (ICS), tasks (interactive toggle/edit), obsidian vault
  browser with note preview, RSS news, yazi-style file manager with preview.
* **Cross-cutting**: 8 themes, settings overlay, debug log page, custom
  widgets, extension loading.

**Not present anywhere in native Vanta:** any notion of a **threshold, alert,
verdict, incident or event log**; any **history that outlives the process**;
any **per-interface** network data; any **socket/connection** table; any
**per-container** detail (only a count); any **git/project** awareness.

### 6.3 Capability matrix

| Capability | Native Vanta | Existing extension | Proposed | Actual gap | Decision |
|---|---|---|---|---|---|
| cpu/mem/disk/net values + trends | Dashboard + Monitor graphs | `system_observatory` (dup) | `system_observatory`, `resource_timeline`, `resource_flow` | **none** | **REMOVE** |
| load average | `status` (1/5/15 + cores) | — | `load_history` | trend graph only; marginal | **REMOVE** (fold per-core normalisation into verdict logic) |
| "is anything wrong?" verdict | **none** — only per-widget colour ramps | `system_health` (new) | `system_health` | **whole capability** | **KEEP → seed of new extension** |
| sustained breach (x% for N min) | none | none | — | whole capability | **BUILD** |
| event log / state-change timeline | **none at all** | none | `event_stream`, `event_timeline`, `event_stats` | whole capability | **BUILD — merge `event_stream` here** |
| incident correlation (what ran during the spike) | none (only top-by-cpu *now*) | none | — | whole capability | **BUILD** |
| process list/sort/search/tree/kill | Monitor: htop-class table | — | `process_explorer` + 5 widgets | **none** | **REMOVE** |
| short-lived process / exec lineage | none (snapshot misses processes that die between samples) | none | — | real gap; needs host collector | **DEFER** (host first) |
| aggregate network rate graphs | Dashboard + Monitor | — | `traffic_graph`, `network_radar` | **none** | **REMOVE** |
| per-interface counters | none (host sums `/proc/net/dev`) | none | `interface_status` | real gap; needs host topic | **DEFER** (host first) |
| socket/connection table + process attribution | none | none | `connection_table` | real gap; **highest-value network work** | **DEFER** (host first) |
| network topology | none | none | `network_topology` | no data source exists on the machine | **REJECT** — cannot be built honestly |
| container count | `status`: `docker N/M` | — | `container_status` | count only | partial |
| per-container resources / logs / images / restarts | none | none | `container_*` | real gap; must beat a `docker ps` custom widget | **DEFER** (host first) |
| git / project intelligence | **none** | none | `developer_workspace`, `git_activity`, `project_status` | whole capability | **DEFER** (host first) — genuinely additive |
| build/test status | none | none | `build_status` | would require executing builds | **REJECT** unless the host grows a collector |
| notes / tasks / agenda / news / files | Workspace page | — | `workspace_stats`, `dev_events` | mostly covered | **REMOVE** overlap |
| CVE / threat feed | none | **`security` (MOCK)** | `security_events/alerts/cves` | needs network, which plugins do not have | **FIX HONESTY NOW**; real version blocked |
| local security posture (listening ports, failed logins, pending security updates) | pkgs+updates count | none | `security_summary` | real gap; needs host topics | **DEFER** (host first) — the honest security capability |
| market / crypto data | none | `crypto_coin` (pure math, claims nothing) | `crypto_chart`, `market_ticker` | needs network | keep coin as **aesthetic**; reject data widgets for now |
| one scalar/text from a command or file | **`custom_widgets`** | — | many | **none** | raises the bar for every extension |

### 6.4 Verdict on `system_observatory`

It **substantially duplicates native Vanta** and must not ship as-is.
Four of its five widgets (`system_observatory`, `resource_timeline`,
`resource_flow`, `load_history`) restate the Dashboard and Monitor pages.

Reusable, do **not** rewrite:
* `system_observatory/src/health.rs` — threshold rules, `Level`, `Finding`,
  per-core load normalisation, "missing data is not a finding", 6 unit tests.
  This is the seed of the one genuinely new capability.
* `system_observatory/src/state.rs` — the shared-snapshot + short-TTL cache
  pattern and its rationale (one host query per frame, panels cannot disagree).
* `vanta-ext-sdk` in full — telemetry client, bounded history, UI primitives.

Discard: the four duplicate widget renderers in `lib.rs`, plus the registry
entry and artifact (never pushed, so no user is affected).

### 6.5 Resulting capability-driven roadmap

**C1 — `sentinel`: threshold watching, incidents and event correlation.**
The only genuinely additive capability that is buildable **today** with the
existing telemetry API. Native Vanta tells you what is happening *now*;
`sentinel` tells you *what changed, when, for how long, and what was running
at the time*. Absorbs the roadmap's `event_stream`/`event_timeline`/
`event_stats`. Needs no host change. Passes both rejection tests: stateful,
cross-metric, time-correlated logic that neither a native widget nor a
command-based custom widget can express.

**C2 — host collectors, in value order.** Each unlocks one extension that is
otherwise impossible to build honestly:
1. `network.connections` (+ process attribution) → connection/flow diagnostics
2. `git` (repo state for a configured path) → developer/project intelligence
3. `containers` (per-container detail via docker/podman) → container operations
4. `security.posture` (listening ports, failed logins, pending security
   updates) → security investigation
5. `network.interfaces` (per-interface counters) → completes #1
6. `process.events` (exec/exit) → short-lived process capture

**C3 — honesty fixes.** `security` presents fabricated CVEs under the title
"Live CVE Feed". That is the exact failure mode these rules exist to prevent
and it ships in the registry today. Either relabel it unmistakably as a demo
or remove the widget until a real source exists.

**Explicitly rejected** (do not revisit without new evidence): duplicate
resource dashboards, duplicate process lists, duplicate network graphs,
`network_topology`, `build_status`, generic "system health page" framings.

---

## Resume Point

**Last completed:**
Capability-gap audit (§6). Verified native Vanta in source, then rejected the
widget-driven roadmap and re-derived it from capabilities. Conclusion:
`system_observatory` duplicates native monitoring in 4 of 5 widgets and must
not ship as built; its `health.rs` is the seed of the one capability Vanta
genuinely lacks.

Earlier in the session: M1 host telemetry API (`vanta` `92d5fbc`), rounded
borders (`vanta` `41482e5`), M2 SDK (`8eff279`), M3 system_observatory
(`96698a1`), docs (`46d440d`).

**Nothing is pushed.** Integrations is 4 commits ahead of origin, host 2.
So the published-but-duplicate `system_observatory` registry entry has never
been installable by a user — it can be removed cleanly.

**Currently working on:**
Nothing in flight. Both trees clean and validated.

**Next action — C0 then C1 (do NOT build `process_explorer`):**

C0, rework in place:
1. `git rm artifacts/system_observatory.wasm`, remove its `registry.json`
   entry (keep `security` and `crypto_coin` entries and hashes untouched).
2. Rename the crate directory to `sentinel/` and the extension id to
   `sentinel`; keep `health.rs` and `state.rs` (they carry the reusable
   rules and the shared-snapshot pattern).
3. Delete the duplicate widget builders from `lib.rs`: `observatory`,
   `timeline`, `flow`, `load_history`. Keep `health_panel` as the basis of
   the verdict widget.

C1, the new capability — `sentinel`:
* Widgets: `sentinel` (current verdict + active incidents), `incidents`
  (timeline of opened/closed breaches with duration), `events` (state-change
  feed), `incident_detail` (the correlation view: what was running when it
  started).
* Logic to add on top of `health.rs`: a `Finding` becomes an **incident** only
  after the breach is sustained for N consecutive samples (debounce, so a
  one-frame spike is not an alert); incidents open, persist and close with a
  duration; each transition emits an event; on open, snapshot the top
  processes from telemetry and keep them with the incident — that is the
  correlation native Vanta cannot give.
* Constraints already known: no host config reaches plugins (§1.3), so
  thresholds ship as sensible constants; history is bounded and dies with the
  process (no persistence — document it, do not fake durability); events must
  be a fixed-capacity ring (§1.9).

**Files to touch (C0/C1):**
- `system_observatory/` → `sentinel/` (`Cargo.toml`, `src/lib.rs`,
  `src/state.rs`, `src/health.rs`, new `src/incident.rs`, `README.md`)
- `Cargo.toml` workspace members, `registry.json`, `artifacts/`
- this document

**Known issues:**
- `cargo clippy --workspace -- -D warnings` fails on 24 pre-existing
  `crypto_coin` lints only; validate new crates with `-p <crate>` (§3).
- Host `cargo run --release --example ...` OOMs (LTO); use debug.
- `rustup target add wasm32-wasip1` reports a conflict but the target works.

**Validation status (end of session, both repos clean):**
- `cargo test -p vanta-ext-sdk` 18 passed; `-p system-observatory` 8 passed
- clippy clean on both new crates; `cargo fmt --all -- --check` clean
- `cargo build --target wasm32-wasip1 --release` all crates build
- registry sha256 verified against all three committed artifacts
- host: 88 tests, release build, live render confirmed in a real session

**Do not redo:**
- The capability probe (§1.3) or the capability audit (§6).
- The host telemetry API — extend it with topics, do not replace it.
- The SDK primitives and the shared-snapshot pattern — reuse them.
- Do **not** build another resource dashboard, process list or network graph;
  §6.3 records the rejections and the reasoning.
- Do not fix `crypto_coin` lints without rebuilding/republishing its artifact
  and sha256 in the same commit.

**User's environment:**
`~/.config/vanta/config.toml` was restored after the live test
(`enabled = ["crypto_coin"]`, original layout). A stale
`system_observatory.wasm` remains in `~/.config/vanta/extensions/` but is not
enabled; delete it during C0.
