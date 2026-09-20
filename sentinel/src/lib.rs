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
use vanta_ext_sdk::viz;
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
    /// Host capability report, fetched once.
    caps: Option<vanta_ext_sdk::telemetry::Capabilities>,
    caps_done: bool,
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
        caps: None,
        caps_done: false,
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

    // Capabilities describe the host build and cannot change while it runs,
    // so this happens exactly once and never on the hot path again.
    let need_caps = STORE.with(|s| s.try_borrow().map(|st| !st.caps_done).unwrap_or(false));
    if need_caps {
        let caps = telemetry::capabilities().ok();
        STORE.with(|s| {
            if let Ok(mut st) = s.try_borrow_mut() {
                st.caps = caps;
                st.caps_done = true;
            }
        });
    }

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
            caps: None,
            caps_done: true,
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
        "incident",
        "incidents",
        "sentinel_events",
        "incident_context",
        "sentinel_coverage",
    ])?)
}

#[plugin_fn]
pub fn render_widget(widget_id: String) -> FnResult<Vec<u8>> {
    let w = with_store(|st| match widget_id.as_str() {
        "sentinel" => overview(st),
        "incident" => detail(st),
        "incidents" => incidents(st),
        "sentinel_events" => events(st),
        "incident_context" => context(st),
        "sentinel_coverage" => coverage(st),
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

// ── Component: live incident header ───────────────────────────────────────

/// `▲ CRITICAL  cpu utilisation   97%  ≥85%   4m12s  persisting`
///
/// Answers severity, condition, value, threshold, duration and state in one
/// line, so it can head any panel without costing vertical space.
fn incident_header(i: &Incident, now: u64) -> Vec<Line> {
    let c = level_color(i.level);
    vec![
        Line::new(vec![
            ui::span("▲ ", Style::fg(c.clone()).bold()),
            ui::span(
                format!("{:<9}", severity_word(i.level)),
                Style::fg(c.clone()).bold(),
            ),
            ui::span(
                format!("{:<18}", ui::truncate(&i.metric, 18)),
                Style::default(),
            ),
            ui::span(i.state.label(), Style::fg(state_color(i.state))),
        ]),
        Line::new(vec![
            ui::span("  ", Style::dim()),
            ui::span(
                format!("{:<8}", fmt_val(&i.key, i.current)),
                Style::fg(c).bold(),
            ),
            ui::span(
                format!("≥{:<8}", fmt_val(&i.key, i.threshold)),
                Style::dim(),
            ),
            ui::span(
                format!("{:<9}", fmt_dur(i.duration_ms(now))),
                Style::default(),
            ),
            ui::span(format!("peak {}", fmt_val(&i.key, i.peak)), Style::dim()),
        ]),
    ]
}

fn severity_word(l: Level) -> &'static str {
    match l {
        Level::Critical => "CRITICAL",
        Level::Warn => "WARNING",
        Level::Ok => "OK",
    }
}

/// Compact single line used in lists: fits a 46-column panel.
fn incident_line(i: &Incident, now: u64) -> Line {
    let c = level_color(i.level);
    Line::new(vec![
        ui::span("▲ ", Style::fg(c.clone()).bold()),
        ui::span(
            format!("{:<15}", ui::truncate(&i.metric, 15)),
            Style::fg(c.clone()),
        ),
        ui::span(
            format!("{:>6}", fmt_val(&i.key, i.current)),
            Style::fg(c).bold(),
        ),
        ui::span(
            format!("{:>8}", fmt_dur(i.duration_ms(now))),
            Style::default(),
        ),
        ui::span(
            format!(" {}", short_state(i.state)),
            Style::fg(state_color(i.state)),
        ),
    ])
}

fn short_state(s: State) -> &'static str {
    match s {
        State::Pending => "det",
        State::Open => "open",
        State::Persisting => "persist",
        State::Closed => "closed",
    }
}

// ── Component: incident state machine ─────────────────────────────────────

const STAGES: [&str; 5] = ["norm", "det", "open", "persist", "recov"];

fn stage_index(s: State) -> usize {
    match s {
        State::Pending => 1,
        State::Open => 2,
        State::Persisting => 3,
        State::Closed => 4,
    }
}

// ── Component: incident pulse ─────────────────────────────────────────────

/// The incident's own progression, independent of the metric's value:
/// detection → open → persistence, with the duration meter beneath.
///
/// Answers "how far through its life is this incident" — a question the
/// signal strip cannot answer because a flat 99% looks the same at 2s and
/// at 20 minutes.
fn incident_pulse(i: &Incident, now: u64, width: usize) -> Vec<Line> {
    let elapsed = i.duration_ms(now);
    let meter = viz::duration_meter(elapsed, &[10, 60, 300, 1800], width.min(20));
    vec![
        viz::state_machine(&STAGES, stage_index(i.state), level_color(i.level)),
        Line::new(
            [
                vec![ui::span(
                    format!("held {:<6}", fmt_dur(elapsed)),
                    Style::fg(level_color(i.level)),
                )],
                meter.spans,
                vec![ui::span(" 30m+", Style::dim())],
            ]
            .concat(),
        ),
    ]
}

// ── Component: threshold / breach gauge ───────────────────────────────────

/// Where the value sits relative to Sentinel's own detection bands.
fn breach_gauge(i: &Incident, width: usize) -> Vec<Line> {
    let scale = watch::scale_for(&i.key);
    let gauge = viz::threshold_gauge(
        i.current,
        i.clear_threshold,
        i.threshold,
        scale,
        width.min(26),
        level_color(i.level),
    );
    let mut out = Vec::with_capacity(2);
    for (n, l) in gauge.into_iter().enumerate() {
        let label = if n == 0 {
            ui::span(
                format!("{:<6}", fmt_val(&i.key, i.current)),
                Style::fg(level_color(i.level)).bold(),
            )
        } else {
            ui::span("      ", Style::dim())
        };
        let mut spans = vec![label];
        spans.extend(l.spans);
        if n == 1 {
            spans.push(ui::span(
                format!(
                    " clr{} trg{}",
                    fmt_val(&i.key, i.clear_threshold),
                    fmt_val(&i.key, i.threshold)
                ),
                Style::dim(),
            ));
        }
        out.push(Line::new(spans));
    }
    out
}

// ── Component: signal strip with temporal markers ─────────────────────────

/// The underlying signal around the incident, with markers for open (▲),
/// peak (◆) and recovery (▼). This is the before / during / after view.
fn signal_block(st: &Store, i: &Incident, width: usize) -> Vec<Line> {
    let Some(sig) = st.watcher.signal(&i.key) else {
        return vec![Line::text("no signal history yet", Style::dim())];
    };
    let vals = sig.values();
    if vals.is_empty() {
        return vec![Line::text("no signal history yet", Style::dim())];
    }
    let mut markers = Vec::new();
    if let Some(o) = sig.opened_at.and_then(|x| sig.offset_of(x)) {
        markers.push(viz::Marker {
            at: o,
            glyph: '▲',
            color: level_color(i.level),
        });
    }
    if let Some(p) = sig.peak_at.and_then(|x| sig.offset_of(x)) {
        markers.push(viz::Marker {
            at: p,
            glyph: '◆',
            color: Color::WHITE,
        });
    }
    if let Some(c) = sig.closed_at.and_then(|x| sig.offset_of(x)) {
        markers.push(viz::Marker {
            at: c,
            glyph: '▼',
            color: Color::GREEN,
        });
    }
    let w = width.min(40);
    let (signal, marks) = viz::signal_strip(
        vals,
        i.threshold,
        w,
        &markers,
        Color::CYAN,
        level_color(i.level),
    );
    let mut sig_line = vec![ui::span("sig    ", Style::dim())];
    sig_line.extend(signal.spans);
    let mut mark_line = vec![ui::span("       ", Style::dim())];
    mark_line.extend(marks.spans);
    vec![
        Line::new(sig_line),
        Line::new(mark_line),
        Line::new(vec![
            ui::span("       ", Style::dim()),
            ui::span(
                format!(
                    "{:<w$}",
                    format!("◀ {} samples", vals.len().min(w)),
                    w = w.saturating_sub(3)
                ),
                Style::dim(),
            ),
            ui::span("now", Style::dim()),
        ]),
        Line::new(vec![
            ui::span("       ", Style::dim()),
            ui::span("▲ opened  ◆ peak  ▼ recovered", Style::dim()),
        ]),
    ]
}

// ── Widget: sentinel (status + active incidents) ──────────────────────────

fn overview(st: &Store) -> Widget {
    if let Some(w) = telemetry_down(st, "sentinel") {
        return w;
    }
    let now = now_ms();
    let eng = &st.watcher.engine;
    let mut lines: Vec<Line> = Vec::with_capacity(14);

    lines.extend(status_bar(st, now));
    if let Some(s) = staleness(st, now) {
        lines.push(Line::text(s, Style::fg(Color::YELLOW)));
    }
    lines.push(Line::blank());

    let mut active: Vec<&Incident> = eng.active().collect();
    active.sort_by(|a, b| b.level.cmp(&a.level).then(a.opened_ms.cmp(&b.opened_ms)));

    if active.is_empty() {
        lines.push(Line::text("no active incidents", Style::fg(Color::GREEN)));
        lines.push(Line::blank());
        match eng.closed().next() {
            Some(i) => {
                let end = i.closed_ms.unwrap_or(now);
                lines.push(Line::new(vec![
                    ui::span("last   ", Style::dim()),
                    ui::raw(format!(
                        "{} recovered after {}",
                        ui::truncate(&i.metric, 18),
                        fmt_dur(i.duration_ms(end))
                    )),
                ]));
                lines.push(Line::new(vec![
                    ui::span("       ", Style::dim()),
                    ui::span(fmt_ago(now, end), Style::dim()),
                ]));
            }
            None => lines.push(Line::text("no incidents since start", Style::dim())),
        }
    } else {
        // Headline incident gets the full header; the rest are compact.
        lines.extend(incident_header(active[0], now));
        for i in active.iter().skip(1).take(5) {
            lines.push(incident_line(i, now));
        }
        lines.extend(incident_pulse(active[0], now, 24));
    }

    lines.push(Line::blank());
    lines.push(activity_line(st, now, 26));

    Widget::paragraph(lines).block(block(st, "sentinel"))
}

/// Two lines so it survives a narrow panel:
/// `● 2 ACTIVE  1 crit 1 warn` / `8 watched · 142 samples · 12m`
fn status_bar(st: &Store, now: u64) -> Vec<Line> {
    let eng = &st.watcher.engine;
    let n = eng.active_count();
    let crit = eng.active().filter(|i| i.level == Level::Critical).count();
    let warn = n - crit;
    let (col, label) = match eng.worst_active_level() {
        None => (Color::GREEN, "NOMINAL".to_string()),
        Some(l) => (level_color(l), format!("{n} ACTIVE")),
    };
    let mut top = vec![
        ui::span("● ", Style::fg(col.clone()).bold()),
        ui::span(format!("{label:<10}"), Style::fg(col).bold()),
    ];
    if crit > 0 {
        top.push(ui::span(format!("{crit} crit "), Style::fg(Color::RED)));
    }
    if warn > 0 {
        top.push(ui::span(format!("{warn} warn"), Style::fg(Color::YELLOW)));
    }
    vec![
        Line::new(top),
        Line::text(
            format!(
                "{} watched · {} samples · {}",
                watched_count(st),
                eng.sample_count,
                fmt_dur(now.saturating_sub(st.started_ms))
            ),
            Style::dim(),
        ),
    ]
}

/// Event activity over the last 5 minutes.
fn activity_line(st: &Store, now: u64, width: usize) -> Line {
    let stamps: Vec<u64> = st.watcher.engine.events().map(|e| e.at_ms).collect();
    Line::new(vec![
        ui::span("activity ", Style::dim()),
        ui::span(
            viz::density_strip(&stamps, now, 300_000, width),
            Style::fg(if stamps.is_empty() {
                Color::DARK_GRAY
            } else {
                Color::CYAN
            }),
        ),
        ui::span("  5m", Style::dim()),
    ])
}

fn watched_count(st: &Store) -> usize {
    let sample = Sample {
        cpu: st.cpu.as_ref(),
        memory: st.memory.as_ref(),
        disk: st.disk.as_ref(),
        processes: None,
    };
    watch::measured(&sample).len()
}

// ── Widget: incident detail (pulse + gauge + signal) ──────────────────────

fn detail(st: &Store) -> Widget {
    if let Some(w) = telemetry_down(st, "incident") {
        return w;
    }
    let now = now_ms();
    let eng = &st.watcher.engine;
    let target = eng
        .active()
        .max_by(|a, b| a.level.cmp(&b.level).then(b.opened_ms.cmp(&a.opened_ms)))
        .or_else(|| eng.closed().next());

    let Some(i) = target else {
        return Widget::Paragraph {
            lines: vec![
                Line::text("no incident to inspect", Style::fg(Color::GREEN)),
                Line::blank(),
                Line::text(
                    "this panel opens on the most severe active incident, or the most recent recovered one",
                    Style::dim(),
                ),
            ],
            block: Some(block(st, "incident")),
            wrap: true,
        };
    };

    // Ordered by value: the host gives extension panels only ~5 inner rows
    // by default, so the header and state must land first and the signal
    // strip is what gets cut when the panel is short.
    let mut lines = incident_header(i, now);
    lines.extend(incident_pulse(i, now, 24));
    lines.extend(breach_gauge(i, 26));
    lines.extend(signal_block(st, i, 30));
    Widget::paragraph(lines).block(block(st, "incident"))
}

// ── Widget: incidents (active + recovered + comparison) ───────────────────

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

        // Duration bars make relative severity-over-time comparable at a
        // glance; the absolute value is printed alongside.
        let longest = closed
            .iter()
            .map(|i| i.duration_ms(i.closed_ms.unwrap_or(now)))
            .max()
            .unwrap_or(1)
            .max(1);
        for i in closed.iter().take(10) {
            let end = i.closed_ms.unwrap_or(now);
            let d = i.duration_ms(end);
            let bar_w = 8usize;
            let filled = ((d as f64 / longest as f64) * bar_w as f64).round() as usize;
            lines.push(Line::new(vec![
                ui::span("· ", Style::dim()),
                ui::span(
                    format!("{:<15}", ui::truncate(&i.metric, 15)),
                    Style::default(),
                ),
                ui::span(
                    format!("{:>6}", fmt_val(&i.key, i.peak)),
                    Style::fg(level_color(i.level)),
                ),
                ui::span(" ", Style::dim()),
                ui::span("▬".repeat(filled), Style::fg(Color::GRAY)),
                ui::span("·".repeat(bar_w - filled), Style::dim()),
                ui::span(format!(" {:>8}", fmt_dur(d)), Style::default()),
                ui::span(format!("  {}", fmt_ago(now, end)), Style::dim()),
            ]));
        }
    }

    if lines.is_empty() {
        return Widget::Paragraph {
            lines: vec![
                Line::text("no incidents recorded", Style::dim()),
                Line::blank(),
                Line::text(
                    "an incident opens when a watched condition stays over its threshold long enough to rule out a spike",
                    Style::dim(),
                ),
            ],
            block: Some(block(st, "incidents")),
            wrap: true,
        };
    }
    Widget::paragraph(lines).block(block(st, "incidents"))
}

// ── Widget: event stream + density ────────────────────────────────────────

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
                    "events are recorded when an incident opens, escalates, persists or recovers",
                    Style::dim(),
                ),
            ],
            block: Some(block(st, "events")),
            wrap: true,
        };
    }

    let mut lines = vec![activity_line(st, now, 26), Line::blank()];
    for e in evs.iter().take(14) {
        let (glyph, c) = match e.kind {
            EventKind::Opened => ('▲', level_color(e.level)),
            EventKind::Escalated => ('▲', Color::RED),
            EventKind::Persisting => ('■', Color::RED),
            EventKind::Closed => ('▼', Color::GREEN),
        };
        let mut spans = vec![
            ui::span(format!("{:>8} ", fmt_ago(now, e.at_ms)), Style::dim()),
            ui::span(format!("{glyph} "), Style::fg(c.clone()).bold()),
            ui::span(format!("{:<9}", e.kind.label()), Style::fg(c.clone())),
            ui::span(
                format!("{:<15}", ui::truncate(&e.metric, 15)),
                Style::default(),
            ),
            ui::span(format!("{:>6}", fmt_val(&e.key, e.value)), Style::fg(c)),
        ];
        if let Some(d) = e.duration_ms {
            spans.push(ui::span(format!("  after {}", fmt_dur(d)), Style::dim()));
        }
        lines.push(Line::new(spans));
    }
    Widget::paragraph(lines).block(block(st, "events"))
}

// ── Widget: process context captured at open ──────────────────────────────

fn context(st: &Store) -> Widget {
    if let Some(w) = telemetry_down(st, "incident context") {
        return w;
    }
    let now = now_ms();
    let eng = &st.watcher.engine;
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
                    "when an incident opens, the processes running at that exact moment are captured and kept with it",
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
                Style::fg(c).bold(),
            ),
            ui::span(i.state.label(), Style::fg(state_color(i.state))),
            ui::span(
                format!("   opened {}", fmt_ago(now, i.opened_ms)),
                Style::dim(),
            ),
        ]),
        Line::blank(),
    ];

    if i.context_unavailable {
        lines.push(Line::text(
            "process telemetry was unavailable when this opened",
            Style::fg(Color::YELLOW),
        ));
        lines.push(Line::text(
            "no context is shown rather than a misleading later snapshot",
            Style::dim(),
        ));
    } else if i.context.is_empty() {
        lines.push(Line::text("no processes captured", Style::dim()));
    } else {
        lines.push(Line::text(
            "processes at the moment it opened",
            Style::dim(),
        ));
        // Ranked by CPU: the metric most incidents correlate with, and the
        // ranking makes the distribution obvious without a table scan.
        let entries: Vec<(String, f64, String)> = i
            .context
            .iter()
            .map(|p| {
                (
                    format!("{} {}", p.pid, ui::truncate(&p.name, 12)),
                    p.cpu_pct,
                    format!("{:>6.1}%  {:>6}", p.cpu_pct, ui::fmt_bytes(p.mem_kb * 1024)),
                )
            })
            .collect();
        lines.extend(viz::ranking_bars(&entries, 19, 12, Color::MAGENTA));
        lines.push(Line::blank());
        lines.push(Line::text(
            "present at the time — correlation, not a proven cause",
            Style::dim(),
        ));
    }

    Widget::Paragraph {
        lines,
        block: Some(block(st, "incident context")),
        wrap: true,
    }
}

// ── Widget: monitoring coverage ───────────────────────────────────────────

/// What Sentinel can actually see, and what it cannot.
///
/// Deliberately prominent: an incident detector that silently monitors less
/// than the user assumes is worse than one that monitors nothing.
fn coverage(st: &Store) -> Widget {
    let now = now_ms();
    let stale = st.watcher.last_sample_ms > 0
        && now.saturating_sub(st.watcher.last_sample_ms) > STALE_AFTER_MS;

    let sample = Sample {
        cpu: st.cpu.as_ref(),
        memory: st.memory.as_ref(),
        disk: st.disk.as_ref(),
        processes: None,
    };
    let live = watch::measured(&sample);

    let mut lines = vec![Line::new(vec![
        ui::span(
            if live.is_empty() {
                "○ NO TELEMETRY"
            } else if stale {
                "◐ STALE"
            } else {
                "● MONITORING"
            },
            Style::fg(if live.is_empty() {
                Color::RED
            } else if stale {
                Color::YELLOW
            } else {
                Color::GREEN
            })
            .bold(),
        ),
        ui::span(
            format!("   {} conditions   1s interval", live.len()),
            Style::dim(),
        ),
    ])];

    if stale {
        lines.push(Line::text(
            format!("last sample {}", fmt_ago(now, st.watcher.last_sample_ms)),
            Style::fg(Color::YELLOW),
        ));
    }
    lines.push(Line::blank());

    // Watched conditions with their current value and trigger.
    for (key, value) in live.iter().take(10) {
        let (trigger, _clear) = watch::thresholds_for(key);
        let breaching = *value >= trigger;
        // Tracked-but-not-breaching means it is inside the hysteresis band,
        // on its way back down — distinct from both healthy and breaching.
        let tracked = st.watcher.engine.by_key(key).is_some();
        let (glyph, col) = if breaching {
            ('▲', Color::RED)
        } else if tracked {
            ('◐', Color::YELLOW)
        } else if *value >= trigger * 0.8 {
            ('●', Color::YELLOW)
        } else {
            ('●', Color::GREEN)
        };
        lines.push(Line::new(vec![
            ui::span(format!("{glyph} "), Style::fg(col.clone())),
            ui::span(
                format!("{:<10}", ui::truncate(&watch::label_for(key), 10)),
                Style::default(),
            ),
            ui::span(format!("{:>7}", fmt_val(key, *value)), Style::fg(col)),
            ui::span(
                format!("  trigger ≥{}", fmt_val(key, trigger)),
                Style::dim(),
            ),
        ]));
    }

    // The honest part: what the host cannot give us.
    if let Some(caps) = &st.caps {
        if !caps.unavailable.is_empty() {
            lines.push(Line::blank());
            lines.push(Line::text(
                "not monitored — host does not collect",
                Style::dim(),
            ));
            for g in caps.unavailable.iter().take(6) {
                lines.push(Line::new(vec![
                    ui::span("○ ", Style::dim()),
                    ui::span(format!("{:<22}", ui::truncate(&g.topic, 22)), Style::dim()),
                ]));
            }
        }
    }

    Widget::Paragraph {
        lines,
        block: Some(block(st, "coverage")),
        wrap: true,
    }
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
