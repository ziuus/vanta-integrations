//! IOWatch — per-process I/O throughput with temporal history.
//!
//! Native Vanta shows aggregate disk stats. IOWatch answers the different
//! question: **which processes are actually doing the I/O, and how is that
//! changing over time?**
//!
//! # Widgets
//!
//! | widget         | answers                                            |
//! |---|---|
//! | `iowatch`      | ranked table + activity bars + sparklines + peak   |
//! | `io_top`       | compact top-5 by total throughput                  |
//! | `io_activity`  | sparkline history for up to 8 tracked processes    |
//!
//! # Architecture
//!
//! One host call per render (topic: `io`), one bounded history ring per
//! tracked process.  The engine is intentionally minimal: no incident state
//! machine, no thresholds — those belong in Sentinel.
//!
//! ## Host budget
//!
//! The host's `render_widget` timeout is **10 ms**. `io` is a single host
//! call; even with a 200-process limit the JSON is typically < 8 KB and
//! deserialises in < 1 ms on the render thread.  One call per render is safe.

use extism_pdk::*;
use std::cell::RefCell;

use vanta_ext_sdk::history::now_ms;
use vanta_ext_sdk::telemetry::{self, IoSnapshot};
use vanta_ext_sdk::ui::{self, Block, Color, Line, Style, Table, Widget};
use vanta_ext_sdk::{ExtensionMetadata, API_VERSION_TELEMETRY};

const ID: &str = "iowatch";
const VERSION: &str = "0.1.0";

/// How many processes to request from the host (ranked by total_bps desc).
const FETCH_LIMIT: usize = 15;
/// Maximum number of processes tracked in history.
const TRACK_MAX: usize = 8;
/// History depth per tracked process (samples, not seconds).
const HISTORY_DEPTH: usize = 40;
/// Minimum ms between fetches — avoids hammering on high-frame-rate hosts.
const REFRESH_MS: u64 = 500;

// ── History ───────────────────────────────────────────────────────────────────

/// Rolling total_bps history for one process.
#[derive(Debug, Clone)]
pub struct ProcHistory {
    pub pid: u32,
    pub name: String,
    pub samples: Vec<f64>,
    /// Peak ever observed for this process.
    pub peak_bps: f64,
}

impl ProcHistory {
    pub fn new(pid: u32, name: &str) -> Self {
        Self {
            pid,
            name: name.to_string(),
            samples: Vec::with_capacity(HISTORY_DEPTH),
            peak_bps: 0.0,
        }
    }

    pub fn push(&mut self, total_bps: f64) {
        if self.samples.len() >= HISTORY_DEPTH {
            self.samples.remove(0);
        }
        self.samples.push(total_bps);
        if total_bps > self.peak_bps {
            self.peak_bps = total_bps;
        }
    }

    pub fn current(&self) -> f64 {
        self.samples.last().copied().unwrap_or(0.0)
    }
}

// ── Shared state ──────────────────────────────────────────────────────────────

pub struct Store {
    pub last: Option<IoSnapshot>,
    pub history: Vec<ProcHistory>,
    pub last_fetch_ms: u64,
    pub io_available: bool,
    pub caps_done: bool,
    pub error: Option<String>,
    pub started_ms: u64,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            last: None,
            history: Vec::new(),
            last_fetch_ms: 0,
            io_available: true,
            caps_done: false,
            error: None,
            started_ms: 0,
        }
    }
}

thread_local! {
    static STORE: RefCell<Store> = RefCell::new(Store::default());
}

// ── Engine ────────────────────────────────────────────────────────────────────

/// Advance state: optionally fetch, update history.
///
/// One host call per render.  `RefCell` borrow dropped before the host call,
/// reacquired after — never held across the boundary.
fn tick() {
    let now = now_ms();

    let is_first = STORE.with(|s| s.borrow().started_ms == 0);
    if is_first {
        STORE.with(|s| s.borrow_mut().started_ms = now);
    }

    // Capabilities check — once only.
    let need_caps = STORE.with(|s| !s.borrow().caps_done);
    if need_caps {
        let caps = telemetry::capabilities();
        STORE.with(|s| {
            let mut st = s.borrow_mut();
            st.caps_done = true;
            if let Ok(c) = caps {
                st.io_available = c.has("io");
            }
        });
    }

    let (due, available) = STORE.with(|s| {
        let st = s.borrow();
        (
            now.saturating_sub(st.last_fetch_ms) >= REFRESH_MS,
            st.io_available,
        )
    });

    if !due || !available {
        return;
    }

    // Fetch — borrow is not held here.
    let result = telemetry::io(FETCH_LIMIT);

    STORE.with(|s| {
        let mut st = s.borrow_mut();
        st.last_fetch_ms = now;
        match result {
            Err(e) => st.error = Some(e.to_string()),
            Ok(snap) => {
                st.error = None;

                // Track top TRACK_MAX processes by total_bps.
                let tracked: Vec<(u32, String, f64)> = snap
                    .processes
                    .iter()
                    .take(TRACK_MAX)
                    .map(|p| (p.pid, p.name.clone(), p.total_bps))
                    .collect();

                for (pid, name, bps) in &tracked {
                    if let Some(h) = st.history.iter_mut().find(|h| h.pid == *pid) {
                        h.name = name.clone();
                        h.push(*bps);
                    } else if st.history.len() < TRACK_MAX {
                        let mut h = ProcHistory::new(*pid, name);
                        h.push(*bps);
                        st.history.push(h);
                    }
                }

                // Tombstone disappeared processes with a zero sample so their
                // sparkline tapers rather than freezing on the last value.
                let live_pids: Vec<u32> = tracked.iter().map(|(p, _, _)| *p).collect();
                for h in &mut st.history {
                    if !live_pids.contains(&h.pid) {
                        h.push(0.0);
                    }
                }

                st.last = Some(snap);
            }
        }
    });
}

// ── Formatting ────────────────────────────────────────────────────────────────

/// Format bytes/s into a human-readable rate string (max 9 chars).
pub fn fmt_rate(bps: f64) -> String {
    if bps < 1024.0 {
        format!("{:>6.0} B/s", bps)
    } else if bps < 1024.0 * 1024.0 {
        format!("{:>5.1} K/s", bps / 1024.0)
    } else if bps < 1024.0 * 1024.0 * 1024.0 {
        format!("{:>5.1} M/s", bps / (1024.0 * 1024.0))
    } else {
        format!("{:>5.1} G/s", bps / (1024.0 * 1024.0 * 1024.0))
    }
}

/// Color thresholds: green > 1 MB/s, yellow > 10 MB/s, red > 50 MB/s.
pub fn rate_color(bps: f64) -> Color {
    if bps > 50.0 * 1024.0 * 1024.0 {
        Color::RED
    } else if bps > 10.0 * 1024.0 * 1024.0 {
        Color::YELLOW
    } else if bps > 1024.0 * 1024.0 {
        Color::GREEN
    } else {
        Color::WHITE
    }
}

/// Scale `value` against `max` and return a horizontal bar string of `width`
/// chars using filled / empty Unicode block characters.
pub fn activity_bar(value: f64, max: f64, width: usize) -> String {
    let fill = if max > 0.0 {
        ((value / max) * width as f64).round() as usize
    } else {
        0
    }
    .min(width);
    format!("{}{}", "█".repeat(fill), "░".repeat(width - fill))
}

// ── Widget helpers ────────────────────────────────────────────────────────────

fn sep(w: u16) -> Line {
    Line::text("─".repeat(w as usize), Style::dim())
}

fn error_widget(title: &str, msg: &str) -> Widget {
    Widget::paragraph(vec![
        Line::text(format!("{title} — error"), Style::fg(Color::RED).bold()),
        Line::text(msg.to_string(), Style::dim()),
    ])
    .block(Block::titled(format!(" {title} ")))
}

fn loading_widget(title: &str, msg: &str) -> Widget {
    Widget::paragraph(vec![Line::text(msg.to_string(), Style::dim())])
        .block(Block::titled(format!(" {title} ")))
}

// ── iowatch (full) ────────────────────────────────────────────────────────────

fn build_iowatch(w: u16, h: u16) -> Widget {
    tick();

    STORE.with(|s| {
        let st = s.borrow();

        if let Some(ref e) = st.error {
            return error_widget("IOWATCH", e);
        }
        let Some(ref snap) = st.last else {
            return loading_widget("IOWATCH", "Collecting I/O data…");
        };

        let mut lines: Vec<Line> = Vec::new();

        // ── Header: total read/write/sum ─────────────────────────────────────
        lines.push(Line::new(vec![
            ui::span("IOWATCH", Style::default().bold()),
            ui::span("  ↓ ", Style::dim()),
            ui::span(fmt_rate(snap.total_read_bps), Style::fg(Color::CYAN)),
            ui::span("  ↑ ", Style::dim()),
            ui::span(fmt_rate(snap.total_write_bps), Style::fg(Color::YELLOW)),
            ui::span("  ∑ ", Style::dim()),
            ui::span(fmt_rate(snap.total_bps()), Style::fg(Color::WHITE).bold()),
        ]));
        lines.push(sep(w));

        if snap.processes.is_empty() {
            lines.push(Line::text("  no disk activity", Style::dim()));
        } else {
            // ── Process table ────────────────────────────────────────────────
            // Fixed-width columns: name(14) read(9) write(9) total(9) bar(rest)
            let bar_w = (w as usize).saturating_sub(46).clamp(6, 20);

            let mut table = Table::new(&[
                ("PROCESS", 14, false),
                ("READ", 9, true),
                ("WRITE", 9, true),
                ("TOTAL", 9, true),
                (&"─".repeat(bar_w), bar_w, false),
            ]);

            let global_max = snap
                .processes
                .iter()
                .map(|p| p.total_bps)
                .fold(0.0_f64, f64::max)
                .max(1.0);

            let table_rows = (h as usize)
                .saturating_sub(8)
                .max(1)
                .min(snap.processes.len());

            for p in snap.processes.iter().take(table_rows) {
                let col = rate_color(p.total_bps);
                let bar = activity_bar(p.total_bps, global_max, bar_w);
                table.row(vec![
                    (p.name.clone(), Style::fg(Color::WHITE)),
                    (fmt_rate(p.read_bps), Style::fg(Color::CYAN)),
                    (fmt_rate(p.write_bps), Style::fg(Color::YELLOW)),
                    (fmt_rate(p.total_bps), Style::fg(col.clone())),
                    (bar, Style::fg(col)),
                ]);
            }

            for l in table.lines() {
                lines.push(l);
            }

            lines.push(sep(w));

            // ── Totals row ───────────────────────────────────────────────────
            lines.push(Line::new(vec![
                ui::span(format!("{:<14}", "TOTAL"), Style::default().bold()),
                ui::span(" ", Style::default()),
                ui::span(
                    format!("{:>9}", fmt_rate(snap.total_read_bps)),
                    Style::fg(Color::CYAN),
                ),
                ui::span(" ", Style::default()),
                ui::span(
                    format!("{:>9}", fmt_rate(snap.total_write_bps)),
                    Style::fg(Color::YELLOW),
                ),
                ui::span(" ", Style::default()),
                ui::span(
                    format!("{:>9}", fmt_rate(snap.total_bps())),
                    Style::fg(Color::GREEN).bold(),
                ),
            ]));

            // ── Activity sparklines ──────────────────────────────────────────
            if h as usize > lines.len() + 4 && !st.history.is_empty() {
                lines.push(Line::blank());
                lines.push(Line::text("ACTIVITY", Style::dim()));

                let spark_w = (w as usize).saturating_sub(16).clamp(8, HISTORY_DEPTH);
                let hist_max = st
                    .history
                    .iter()
                    .flat_map(|h| h.samples.iter().copied())
                    .fold(0.0_f64, f64::max)
                    .max(1.0);

                let rows_avail = (h as usize).saturating_sub(lines.len() + 3).max(1);
                for hist in st.history.iter().take(rows_avail) {
                    lines.push(ui::metric_row(
                        &hist.name,
                        14,
                        &hist.samples,
                        spark_w,
                        Some(hist_max),
                        &fmt_rate(hist.current()),
                        rate_color(hist.current()),
                    ));
                }

                // Peak line
                let peak_proc = st
                    .history
                    .iter()
                    .max_by(|a, b| a.peak_bps.total_cmp(&b.peak_bps));
                if let Some(pk) = peak_proc {
                    if (h as usize) > lines.len() + 1 {
                        lines.push(Line::new(vec![
                            ui::span("PEAK  ", Style::dim()),
                            ui::span(pk.name.clone(), Style::fg(Color::WHITE)),
                            ui::span("  ", Style::default()),
                            ui::span(fmt_rate(pk.peak_bps), Style::fg(Color::RED).bold()),
                        ]));
                    }
                }
            }
        }

        Widget::paragraph(lines).block(Block::titled(" IOWATCH ".to_string()))
    })
}

// ── io_top (compact) ─────────────────────────────────────────────────────────

fn build_io_top(w: u16, h: u16) -> Widget {
    tick();

    STORE.with(|s| {
        let st = s.borrow();

        if let Some(ref e) = st.error {
            return error_widget("IO TOP", e);
        }
        let Some(ref snap) = st.last else {
            return loading_widget("IO TOP", "waiting…");
        };

        let global_max = snap
            .processes
            .iter()
            .map(|p| p.total_bps)
            .fold(0.0_f64, f64::max)
            .max(1.0);

        let bar_w = (w as usize).saturating_sub(26).clamp(4, 16);
        let rows = (h as usize).saturating_sub(2).clamp(1, 5);

        let mut lines = vec![Line::new(vec![
            ui::span("IO TOP ", Style::default().bold()),
            ui::span(fmt_rate(snap.total_bps()), Style::fg(Color::CYAN)),
        ])];

        if snap.processes.is_empty() {
            lines.push(Line::text("  idle", Style::dim()));
        } else {
            for p in snap.processes.iter().take(rows) {
                let col = rate_color(p.total_bps);
                let bar = activity_bar(p.total_bps, global_max, bar_w);
                lines.push(Line::new(vec![
                    ui::span(
                        format!("{:<12}", ui::truncate(&p.name, 12)),
                        Style::fg(Color::WHITE),
                    ),
                    ui::span(" ", Style::default()),
                    ui::span(
                        format!("{:>9}", fmt_rate(p.total_bps)),
                        Style::fg(col.clone()),
                    ),
                    ui::span(" ", Style::default()),
                    ui::span(bar, Style::fg(col)),
                ]));
            }
        }

        Widget::paragraph(lines).block(Block::titled(" IO TOP ".to_string()))
    })
}

// ── io_activity (sparklines only) ────────────────────────────────────────────

fn build_io_activity(w: u16, h: u16) -> Widget {
    tick();

    STORE.with(|s| {
        let st = s.borrow();

        if let Some(ref e) = st.error {
            return error_widget("IO ACTIVITY", e);
        }
        if st.history.is_empty() {
            return loading_widget("IO ACTIVITY", "collecting…");
        }

        let spark_w = (w as usize).saturating_sub(16).clamp(8, HISTORY_DEPTH);
        let hist_max = st
            .history
            .iter()
            .flat_map(|h| h.samples.iter().copied())
            .fold(0.0_f64, f64::max)
            .max(1.0);

        let rows = (h as usize).max(1);
        let lines: Vec<Line> = st
            .history
            .iter()
            .take(rows)
            .map(|hist| {
                ui::metric_row(
                    &hist.name,
                    12,
                    &hist.samples,
                    spark_w,
                    Some(hist_max),
                    &fmt_rate(hist.current()),
                    rate_color(hist.current()),
                )
            })
            .collect();

        Widget::paragraph(lines).block(Block::titled(" IO ACTIVITY ".to_string()))
    })
}

// ── Plugin exports ────────────────────────────────────────────────────────────


#[plugin_fn]
pub fn metadata() -> FnResult<Vec<u8>> {
    Ok(vanta_ext_sdk::ExtensionMetadata::new(
        "iowatch_activity",
        "Iowatch Activity",
        "0.1.0",
        "Micro-extension",
        vanta_ext_sdk::API_VERSION_TELEMETRY,
    )
    .to_json())
}

#[plugin_fn]
pub fn widgets(_: ()) -> FnResult<Vec<u8>> {
    let ids = serde_json::json!(["io_activity"]);
    Ok(serde_json::to_vec(&ids).unwrap_or_default())
}

#[plugin_fn]
pub fn render_widget(id: String) -> FnResult<Vec<u8>> {
    if id != "io_activity" {
        return Ok(vanta_ext_sdk::ui::unavailable("UNKNOWN", "invalid widget").to_json());
    }
    let widget = build_io_activity(80, 24);
    Ok(widget.to_json())
}
