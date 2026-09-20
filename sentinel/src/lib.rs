//! Sentinel — incident detection and correlation for Vanta.
//!
//! Native Vanta shows what is happening now. Sentinel answers the questions
//! it cannot: did something abnormal start, when, is it still going, how bad
//! did it get, what was running when it began, and when did it recover.
//!
//! Four widgets over one shared engine:
//!
//! | widget | answers |
//! |---|---|
//! | `sentinel` | is anything wrong right now, and for how long |
//! | `incidents` | recent incidents with duration and peak |
//! | `sentinel_events` | chronological feed of state changes |
//! | `incident_context` | which processes were running when it started |
//!
//! Temporal state lives in [`watch::Watcher`]; widgets only read it.

pub mod health;
pub mod incident;
pub mod watch;

use extism_pdk::*;
use std::cell::RefCell;

use vanta_ext_sdk::history::now_ms;
use vanta_ext_sdk::telemetry::{self, Cpu, Disk, Memory, Process};
use vanta_ext_sdk::ui::{self, Block, Color, Line, Style, Widget};
use vanta_ext_sdk::{ExtensionMetadata, API_VERSION_TELEMETRY};

use health::Level;
use incident::{Event, EventKind, Incident, State};
use watch::{Sample, Watcher};

const ID: &str = "sentinel";
const VERSION: &str = "0.1.0";

/// Minimum spacing between engine steps. Independent of frame rate.
const EVAL_INTERVAL_MS: u64 = 1000;
/// Telemetry older than this is called out as stale.
const STALE_AFTER_MS: u64 = 10_000;
/// Processes requested for incident context.
const CONTEXT_FETCH: usize = 8;

// ── Shared state ──────────────────────────────────────────────────────────

#[derive(Default)]
struct Fetched {
    cpu: Option<Cpu>,
    memory: Option<Memory>,
    disk: Option<Disk>,
    procs: Option<Vec<Process>>,
    error: Option<String>,
}

struct Store {
    watcher: Watcher,
    /// Telemetry being accumulated for the next engine step.
    pending: Fetched,
    /// Which topic to fetch on the next render call.
    cursor: u8,
    last_eval_ms: u64,
    /// Latest applied telemetry, for the header readouts.
    cpu: Option<Cpu>,
    memory: Option<Memory>,
    disk: Option<Disk>,
    error: Option<String>,
    started_ms: u64,
}

thread_local! {
    static STORE: RefCell<Store> = RefCell::new(Store {
        watcher: Watcher::new(),
        pending: Fetched { cpu: None, memory: None, disk: None, procs: None, error: None },
        cursor: 0,
        last_eval_ms: 0,
        cpu: None,
        memory: None,
        disk: None,
        error: None,
        started_ms: 0,
    });
}

/// Number of topics gathered before the engine steps.
const TOPICS: u8 = 4;

/// Advance telemetry collection and the engine, then read state.
///
/// Two hard constraints from the host shape this:
///
/// * `render_widget` has a **10 ms timeout**. Fetching four topics in one
///   call exceeds it under load, and a timed-out call is aborted mid-flight.
///   So exactly **one topic is fetched per call**, round-robin; the engine
///   steps once a full set is in hand and the interval has elapsed.
/// * A `RefCell` borrow must **never** be held across a host call. If the
///   timeout fires while a borrow is live the guard never drops and every
///   later call aborts with `already_borrowed`. Host calls therefore happen
///   with no borrow held, and borrows are taken only to read or apply.
fn with_store<R>(f: impl FnOnce(&Store) -> R) -> R {
    let now = now_ms();

    // Decide what to do while holding only a short immutable borrow.
    let (cursor, due) = STORE.with(|s| match s.try_borrow() {
        Ok(st) => (
            st.cursor,
            now.saturating_sub(st.last_eval_ms) >= EVAL_INTERVAL_MS,
        ),
        Err(_) => (0, false),
    });

    if due {
        // ── no borrow held across these host calls ──
        let mut slot = Fetched::default();
        match cursor {
            0 => match telemetry::cpu() {
                Ok(v) => slot.cpu = Some(v),
                Err(e) => slot.error = Some(e.to_string()),
            },
            1 => match telemetry::memory() {
                Ok(v) => slot.memory = Some(v),
                Err(e) => slot.error = Some(e.to_string()),
            },
            2 => match telemetry::disk() {
                Ok(v) => slot.disk = Some(v),
                Err(e) => slot.error = Some(e.to_string()),
            },
            _ => match telemetry::processes(CONTEXT_FETCH) {
                Ok(v) => slot.procs = Some(v.processes),
                Err(e) => slot.error = Some(e.to_string()),
            },
        }

        // ── short mutable borrow, no host calls inside ──
        STORE.with(|s| {
            if let Ok(mut st) = s.try_borrow_mut() {
                if st.started_ms == 0 {
                    st.started_ms = now;
                }
                if slot.cpu.is_some() {
                    st.pending.cpu = slot.cpu;
                }
                if slot.memory.is_some() {
                    st.pending.memory = slot.memory;
                }
                if slot.disk.is_some() {
                    st.pending.disk = slot.disk;
                }
                if slot.procs.is_some() {
                    st.pending.procs = slot.procs;
                }
                if slot.error.is_some() && st.pending.error.is_none() {
                    st.pending.error = slot.error;
                }

                st.cursor = (st.cursor + 1) % TOPICS;
                // A full round has been gathered: step the engine.
                if st.cursor == 0 {
                    st.last_eval_ms = now;
                    apply(&mut st, now);
                }
            }
        });
    }

    STORE.with(|s| match s.try_borrow() {
        Ok(st) => f(&st),
        // Unreachable in practice; degrade instead of aborting the plugin.
        Err(_) => f(&Store {
            watcher: Watcher::new(),
            pending: Fetched::default(),
            cursor: 0,
            last_eval_ms: 0,
            cpu: None,
            memory: None,
            disk: None,
            error: Some("state busy".into()),
            started_ms: now,
        }),
    })
}

/// Move the gathered round into current state and step the engine.
fn apply(st: &mut Store, now: u64) {
    let p = std::mem::take(&mut st.pending);
    st.cpu = p.cpu;
    st.memory = p.memory;
    st.disk = p.disk;
    st.error = p.error;

    // A total telemetry outage must not read as "everything recovered":
    // skip the step so open incidents keep their state.
    if st.cpu.is_none() && st.memory.is_none() && st.disk.is_none() {
        return;
    }
    let sample = Sample {
        cpu: st.cpu.as_ref(),
        memory: st.memory.as_ref(),
        disk: st.disk.as_ref(),
        processes: p.procs.as_deref(),
    };
    st.watcher.ingest(now, &sample);
}

// ── Plugin exports ────────────────────────────────────────────────────────

#[plugin_fn]
pub fn metadata() -> FnResult<Vec<u8>> {
    Ok(ExtensionMetadata::new(
        ID,
        "Sentinel",
        VERSION,
        "Detects when system behaviour goes abnormal, tracks the incident and captures what was running.",
        API_VERSION_TELEMETRY,
    )
    .to_json())
}

#[plugin_fn]
pub fn widgets() -> FnResult<Vec<u8>> {
    Ok(serde_json::to_vec(&[
        "sentinel",
        "incidents",
        "sentinel_events",
        "incident_context",
    ])?)
}

#[plugin_fn]
pub fn render_widget(widget_id: String) -> FnResult<Vec<u8>> {
    let w = with_store(|st| match widget_id.as_str() {
        "sentinel" => overview(st),
        "incidents" => incidents(st),
        "sentinel_events" => events(st),
        "incident_context" => context(st),
        other => ui::unavailable("sentinel", &format!("no widget named '{other}'")),
    });
    Ok(w.to_json())
}

// ── Formatting helpers ────────────────────────────────────────────────────

fn level_color(l: Level) -> Color {
    match l {
        Level::Ok => Color::GREEN,
        Level::Warn => Color::YELLOW,
        Level::Critical => Color::RED,
    }
}

fn state_color(s: State) -> Color {
    match s {
        State::Closed => Color::DARK_GRAY,
        State::Persisting => Color::RED,
        _ => Color::YELLOW,
    }
}

/// Compact duration: `4s`, `2m10s`, `1h04m`.
fn fmt_dur(ms: u64) -> String {
    let s = ms / 1000;
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m{:02}s", s / 60, s % 60)
    } else {
        format!("{}h{:02}m", s / 3600, (s % 3600) / 60)
    }
}

/// Relative age: `now`, `12s ago`, `3m ago`.
fn fmt_ago(now: u64, then: u64) -> String {
    let d = now.saturating_sub(then) / 1000;
    if d < 2 {
        "now".into()
    } else if d < 60 {
        format!("{d}s ago")
    } else if d < 3600 {
        format!("{}m ago", d / 60)
    } else {
        format!("{}h ago", d / 3600)
    }
}

fn fmt_val(key: &str, v: f64) -> String {
    match watch::unit_for(key) {
        "" => format!("{v:.2}"),
        u => format!("{v:.0}{u}"),
    }
}

fn block(st: &Store, title: &str) -> Block {
    let c = st
        .watcher
        .engine
        .worst_active_level()
        .map(level_color)
        .unwrap_or(Color::DARK_GRAY);
    Block::titled(format!(" {title} ")).color(c)
}

/// Shown when telemetry is entirely unavailable. Incident state is retained
/// and said so, rather than implying recovery.
fn telemetry_down(st: &Store, title: &str) -> Option<Widget> {
    let e = st.error.as_ref()?;
    if st.cpu.is_some() || st.memory.is_some() || st.disk.is_some() {
        return None;
    }
    let mut lines = vec![
        Line::text("telemetry unavailable", Style::fg(Color::YELLOW).bold()),
        Line::text(e.clone(), Style::dim()),
    ];
    let active = st.watcher.engine.active_count();
    if active > 0 {
        lines.push(Line::blank());
        lines.push(Line::text(
            format!("{active} incident(s) held, not evaluated"),
            Style::dim(),
        ));
    }
    Some(Widget::Paragraph {
        lines,
        block: Some(Block::titled(format!(" {title} ")).color(Color::DARK_GRAY)),
        wrap: true,
    })
}

/// True when the last successful evaluation is old.
fn staleness(st: &Store, now: u64) -> Option<String> {
    let last = st.watcher.last_sample_ms;
    (last > 0 && now.saturating_sub(last) > STALE_AFTER_MS)
        .then(|| format!("stale · last sample {}", fmt_ago(now, last)))
}

// ── sentinel: current state ───────────────────────────────────────────────

fn overview(st: &Store) -> Widget {
    if let Some(w) = telemetry_down(st, "sentinel") {
        return w;
    }
    let now = now_ms();
    let eng = &st.watcher.engine;
    let mut lines: Vec<Line> = Vec::with_capacity(10);

    let worst = eng.worst_active_level();
    let (dot, label, col) = match worst {
        None => ("●", "NOMINAL".to_string(), Color::GREEN),
        Some(l) => (
            "●",
            format!(
                "{} INCIDENT{}",
                eng.active_count(),
                if eng.active_count() == 1 { "" } else { "S" }
            ),
            level_color(l),
        ),
    };
    lines.push(Line::new(vec![
        ui::span(format!("{dot} "), Style::fg(col.clone()).bold()),
        ui::span(label, Style::fg(col).bold()),
        ui::span(
            format!("   watching {} conditions", watched_count(st)),
            Style::dim(),
        ),
    ]));

    if let Some(s) = staleness(st, now) {
        lines.push(Line::text(s, Style::fg(Color::YELLOW)));
    }

    if eng.active_count() == 0 {
        lines.push(Line::blank());
        let recent = eng.closed().next();
        match recent {
            Some(i) => lines.push(Line::new(vec![
                ui::span("last  ", Style::dim()),
                ui::raw(format!(
                    "{} recovered after {}, {}",
                    i.metric,
                    fmt_dur(i.duration_ms(i.closed_ms.unwrap_or(now))),
                    fmt_ago(now, i.closed_ms.unwrap_or(now))
                )),
            ])),
            None => lines.push(Line::text("no incidents since start", Style::dim())),
        }
        lines.push(Line::blank());
        lines.push(Line::text(
            format!(
                "{} samples · {}",
                eng.sample_count,
                fmt_dur(now.saturating_sub(st.started_ms))
            ),
            Style::dim(),
        ));
        return Widget::paragraph(lines).block(block(st, "sentinel"));
    }

    lines.push(Line::blank());
    // Active incidents, worst first, then longest running.
    let mut active: Vec<&Incident> = eng.active().collect();
    active.sort_by(|a, b| b.level.cmp(&a.level).then(a.opened_ms.cmp(&b.opened_ms)));
    for i in active.iter().take(6) {
        lines.push(incident_line(i, now));
    }
    Widget::paragraph(lines).block(block(st, "sentinel"))
}

/// `▲ crit  cpu utilisation   99% (peak 99%)  4m12s  persisting`
fn incident_line(i: &Incident, now: u64) -> Line {
    let c = level_color(i.level);
    let mark = match i.level {
        Level::Critical => "▲",
        _ => "▲",
    };
    Line::new(vec![
        ui::span(format!("{mark} "), Style::fg(c.clone()).bold()),
        ui::span(
            format!("{:<16}", ui::truncate(&i.metric, 16)),
            Style::fg(c.clone()),
        ),
        ui::span(
            format!("{:>6}", fmt_val(&i.key, i.current)),
            Style::fg(c).bold(),
        ),
        ui::span(
            format!(" peak {:>6}", fmt_val(&i.key, i.peak)),
            Style::dim(),
        ),
        ui::span(
            format!("  {:>7}", fmt_dur(i.duration_ms(now))),
            Style::default(),
        ),
        ui::span(
            format!("  {}", i.state.label()),
            Style::fg(state_color(i.state)),
        ),
    ])
}

fn watched_count(st: &Store) -> usize {
    let mut n = 0;
    if st.cpu.is_some() {
        n += 2; // cpu, load
        if st.cpu.as_ref().and_then(|c| c.max_temp_c).is_some() {
            n += 1;
        }
    }
    if let Some(m) = &st.memory {
        n += 1;
        if m.swap_total_bytes > 0 {
            n += 1;
        }
    }
    n += st.disk.as_ref().map(|d| d.mounts.len()).unwrap_or(0);
    n
}

// ── incidents: active + recent history ────────────────────────────────────

fn incidents(st: &Store) -> Widget {
    if let Some(w) = telemetry_down(st, "incidents") {
        return w;
    }
    let now = now_ms();
    let eng = &st.watcher.engine;
    let mut lines: Vec<Line> = Vec::new();

    let mut active: Vec<&Incident> = eng.active().collect();
    active.sort_by(|a, b| b.level.cmp(&a.level).then(a.opened_ms.cmp(&b.opened_ms)));

    if !active.is_empty() {
        lines.push(Line::text("active", Style::dim()));
        for i in &active {
            lines.push(incident_line(i, now));
        }
    }

    let closed: Vec<&Incident> = eng.closed().collect();
    if !closed.is_empty() {
        if !lines.is_empty() {
            lines.push(Line::blank());
        }
        lines.push(Line::text("recovered", Style::dim()));
        for i in closed.iter().take(10) {
            let end = i.closed_ms.unwrap_or(now);
            lines.push(Line::new(vec![
                ui::span("· ", Style::dim()),
                ui::span(
                    format!("{:<16}", ui::truncate(&i.metric, 16)),
                    Style::default(),
                ),
                ui::span(
                    format!("{:>6}", fmt_val(&i.key, i.peak)),
                    Style::fg(level_color(i.level)),
                ),
                ui::span(" peak       ", Style::dim()),
                ui::span(
                    format!("{:>7}", fmt_dur(i.duration_ms(end))),
                    Style::default(),
                ),
                ui::span(format!("  {}", fmt_ago(now, end)), Style::dim()),
            ]));
        }
    }

    if lines.is_empty() {
        lines.push(Line::text("no incidents recorded", Style::dim()));
        lines.push(Line::blank());
        lines.push(Line::text(
            "an incident opens when a watched condition stays over threshold",
            Style::dim(),
        ));
        return Widget::Paragraph {
            lines,
            block: Some(block(st, "incidents")),
            wrap: true,
        };
    }
    Widget::paragraph(lines).block(block(st, "incidents"))
}

// ── sentinel_events: transition feed ──────────────────────────────────────

fn events(st: &Store) -> Widget {
    if let Some(w) = telemetry_down(st, "events") {
        return w;
    }
    let now = now_ms();
    let evs: Vec<&Event> = st.watcher.engine.events().collect();
    if evs.is_empty() {
        return Widget::Paragraph {
            lines: vec![
                Line::text("no state changes yet", Style::dim()),
                Line::blank(),
                Line::text(
                    "events appear when an incident opens, escalates, persists or recovers",
                    Style::dim(),
                ),
            ],
            block: Some(block(st, "events")),
            wrap: true,
        };
    }
    let lines = evs
        .iter()
        .take(16)
        .map(|e| {
            let c = match e.kind {
                EventKind::Closed => Color::GREEN,
                EventKind::Escalated => Color::RED,
                _ => level_color(e.level),
            };
            let mut spans = vec![
                ui::span(format!("{:>8} ", fmt_ago(now, e.at_ms)), Style::dim()),
                ui::span(
                    format!("{:<10}", e.kind.label()),
                    Style::fg(c.clone()).bold(),
                ),
                ui::span(
                    format!("{:<16}", ui::truncate(&e.metric, 16)),
                    Style::default(),
                ),
                ui::span(format!("{:>6}", fmt_val(&e.key, e.value)), Style::fg(c)),
            ];
            if let Some(d) = e.duration_ms {
                spans.push(ui::span(format!("  after {}", fmt_dur(d)), Style::dim()));
            }
            Line::new(spans)
        })
        .collect();
    Widget::paragraph(lines).block(block(st, "events"))
}

// ── incident_context: what was running when it started ────────────────────

fn context(st: &Store) -> Widget {
    if let Some(w) = telemetry_down(st, "incident context") {
        return w;
    }
    let now = now_ms();
    let eng = &st.watcher.engine;
    // Prefer the worst active incident; fall back to the most recent closed
    // one so the panel stays useful after recovery.
    let target = eng
        .active()
        .max_by(|a, b| a.level.cmp(&b.level).then(b.opened_ms.cmp(&a.opened_ms)))
        .or_else(|| eng.closed().next());

    let Some(i) = target else {
        return Widget::Paragraph {
            lines: vec![
                Line::text("no incident to correlate", Style::dim()),
                Line::blank(),
                Line::text(
                    "when one opens, the processes running at that moment are captured here",
                    Style::dim(),
                ),
            ],
            block: Some(block(st, "incident context")),
            wrap: true,
        };
    };

    let c = level_color(i.level);
    let mut lines = vec![
        Line::new(vec![
            ui::span(
                format!("{} ", ui::truncate(&i.metric, 18)),
                Style::fg(c.clone()).bold(),
            ),
            ui::span(i.state.label(), Style::fg(state_color(i.state))),
        ]),
        Line::new(vec![
            ui::span("opened  ", Style::dim()),
            ui::raw(fmt_ago(now, i.opened_ms)),
            ui::span("   held ", Style::dim()),
            ui::raw(fmt_dur(i.duration_ms(i.closed_ms.unwrap_or(now)))),
        ]),
        Line::new(vec![
            ui::span("trigger ", Style::dim()),
            ui::raw(format!(
                "≥{}  clear <{}  peak {}",
                fmt_val(&i.key, i.threshold),
                fmt_val(&i.key, i.clear_threshold),
                fmt_val(&i.key, i.peak)
            )),
        ]),
        Line::blank(),
    ];

    if i.context_unavailable {
        lines.push(Line::text(
            "process telemetry was unavailable when this opened",
            Style::fg(Color::YELLOW),
        ));
    } else if i.context.is_empty() {
        lines.push(Line::text("no processes captured", Style::dim()));
    } else {
        lines.push(Line::text("running when it started", Style::dim()));
        let mut t = ui::Table::new(&[
            ("pid", 7, true),
            ("process", 14, false),
            ("cpu%", 7, true),
            ("rss", 6, true),
        ]);
        for p in &i.context {
            t.row(vec![
                (p.pid.to_string(), Style::dim()),
                (p.name.clone(), Style::default()),
                (
                    format!("{:.1}", p.cpu_pct),
                    Style::fg(Color::usage(p.cpu_pct.min(100.0))),
                ),
                (ui::fmt_bytes(p.mem_kb * 1024), Style::dim()),
            ]);
        }
        lines.extend(t.lines());
        lines.push(Line::blank());
        // Correlation, not causation — say so plainly.
        lines.push(Line::text(
            "correlated by time, not a proven cause",
            Style::dim(),
        ));
    }

    Widget::paragraph(lines).block(block(st, "incident context"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_formatting_reads_naturally() {
        assert_eq!(fmt_dur(0), "0s");
        assert_eq!(fmt_dur(4_000), "4s");
        assert_eq!(fmt_dur(130_000), "2m10s");
        assert_eq!(fmt_dur(3_840_000), "1h04m");
    }

    #[test]
    fn relative_time_reads_naturally() {
        assert_eq!(fmt_ago(1_000, 1_000), "now");
        assert_eq!(fmt_ago(13_000, 1_000), "12s ago");
        assert_eq!(fmt_ago(200_000, 1_000), "3m ago");
        assert_eq!(fmt_ago(7_300_000, 1_000), "2h ago");
    }

    #[test]
    fn values_carry_their_unit() {
        assert_eq!(fmt_val("cpu", 99.4), "99%");
        assert_eq!(fmt_val("thermal", 88.0), "88°C");
        assert_eq!(fmt_val("load", 1.25), "1.25");
    }
}
