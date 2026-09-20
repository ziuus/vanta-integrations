//! System Observatory — a coherent control room for the machine, built
//! entirely on telemetry Vanta already collects (`vanta_query`).
//!
//! Five widgets that share one cached snapshot so they can never disagree:
//!
//! | widget | purpose |
//! |---|---|
//! | `system_observatory` | everything at a glance: cpu, mem, disk, net, load, procs, verdict |
//! | `system_health` | what is wrong, and why |
//! | `resource_timeline` | rolling multi-metric history |
//! | `resource_flow` | braille flow of cpu / mem / net |
//! | `load_history` | load average trend, normalised per core |
//!
//! Layout notes: a widget cannot query its own area, so every panel is built
//! from short labels and fixed-width graph columns that stay readable in a
//! narrow dashboard cell and simply leave space on the right when wide.

mod health;
mod state;

use extism_pdk::*;
use vanta_ext_sdk::telemetry::TelemetryError;
use vanta_ext_sdk::ui::{self, Block, Color, Line, Style, Widget};
use vanta_ext_sdk::{ExtensionMetadata, API_VERSION_TELEMETRY};

use health::Level;
use state::State;

const ID: &str = "system_observatory";
const VERSION: &str = "0.1.0";

/// Graph column width. Sized for a three-column dashboard on an 80-wide
/// terminal (~24 usable chars inside a bordered panel).
const SPARK_W: usize = 18;
/// Wider graphs for the panels that are usually given a full row.
const WIDE_W: usize = 40;
const LABEL_W: usize = 6;

#[plugin_fn]
pub fn metadata() -> FnResult<Vec<u8>> {
    Ok(ExtensionMetadata::new(
        ID,
        "System Observatory",
        VERSION,
        "Unified system control room: resources, trends and health.",
        API_VERSION_TELEMETRY,
    )
    .to_json())
}

#[plugin_fn]
pub fn widgets() -> FnResult<Vec<u8>> {
    Ok(serde_json::to_vec(&[
        "system_observatory",
        "system_health",
        "resource_timeline",
        "resource_flow",
        "load_history",
    ])?)
}

#[plugin_fn]
pub fn render_widget(widget_id: String) -> FnResult<Vec<u8>> {
    let w = state::with(|st| match widget_id.as_str() {
        "system_observatory" => observatory(st),
        "system_health" => health_panel(st),
        "resource_timeline" => timeline(st),
        "resource_flow" => flow(st),
        "load_history" => load_history(st),
        other => ui::unavailable(
            "system observatory",
            &format!("no widget named '{other}' in this extension"),
        ),
    });
    Ok(w.to_json())
}

// ── Shared pieces ─────────────────────────────────────────────────────────

fn level_color(l: Level) -> Color {
    match l {
        Level::Ok => Color::GREEN,
        Level::Warn => Color::YELLOW,
        Level::Critical => Color::RED,
    }
}

/// Panel title carrying the health dot, so a glance at the border is enough.
fn titled(st: &State, title: &str) -> Block {
    let findings = health::assess(
        st.latest.cpu.as_ref(),
        st.latest.memory.as_ref(),
        st.latest.disk.as_ref(),
    );
    let level = health::overall(&findings);
    Block::titled(format!(" {title} ")).color(level_color(level))
}

/// Rendered when the host cannot answer. Never falls back to placeholder
/// numbers — an empty graph is honest, a fake one is not.
fn offline(err: &str) -> Widget {
    ui::unavailable(
        "system observatory",
        &format!("host telemetry unavailable — {err}"),
    )
}

fn metric(
    label: &str,
    series: &[f64],
    width: usize,
    max: Option<f64>,
    value: String,
    pct: f64,
) -> Line {
    ui::metric_row(
        label,
        LABEL_W,
        series,
        width,
        max,
        &value,
        Color::usage(pct),
    )
}

// ── system_observatory ────────────────────────────────────────────────────

fn observatory(st: &State) -> Widget {
    if let Some(e) = &st.latest.error {
        if st.latest.cpu.is_none() {
            return offline(e);
        }
    }
    let mut lines: Vec<Line> = Vec::with_capacity(12);

    if let Some(c) = &st.latest.cpu {
        lines.push(metric(
            "cpu",
            &st.cpu_h.recent(SPARK_W),
            SPARK_W,
            Some(100.0),
            format!("{:>3.0}%", c.usage_pct),
            c.usage_pct,
        ));
    }
    if let Some(m) = &st.latest.memory {
        lines.push(metric(
            "mem",
            &st.mem_h.recent(SPARK_W),
            SPARK_W,
            Some(100.0),
            format!("{:>3.0}%", m.used_pct),
            m.used_pct,
        ));
    }
    if let Some(d) = &st.latest.disk {
        if let Some(pct) = state::root_mount(d) {
            lines.push(metric(
                "disk",
                &st.disk_h.recent(SPARK_W),
                SPARK_W,
                Some(100.0),
                format!("{pct:>3.0}%"),
                pct,
            ));
        }
    }
    if let Some(n) = &st.latest.network {
        // Network has no ceiling, so both directions share the window peak —
        // that keeps their relative magnitude readable.
        let peak = st.rx_h.max().max(st.tx_h.max()).max(1.0);
        lines.push(ui::metric_row(
            "net ↓",
            LABEL_W,
            &st.rx_h.recent(SPARK_W),
            SPARK_W,
            Some(peak),
            &ui::fmt_kbps(n.rx_kbps),
            Color::CYAN,
        ));
        lines.push(ui::metric_row(
            "net ↑",
            LABEL_W,
            &st.tx_h.recent(SPARK_W),
            SPARK_W,
            Some(peak),
            &ui::fmt_kbps(n.tx_kbps),
            Color::BLUE,
        ));
    }

    lines.push(Line::blank());

    if let Some(c) = &st.latest.cpu {
        let per_core = c.load1 / c.core_count.max(1) as f64;
        let mut spans = vec![
            ui::span(format!("{:<w$}", "load", w = LABEL_W), Style::dim()),
            ui::span(
                format!("{:.2} {:.2} {:.2}", c.load1, c.load5, c.load15),
                Style::fg(Color::usage(per_core * 100.0)),
            ),
            ui::span(format!("  /{}", c.core_count), Style::dim()),
        ];
        if let Some(t) = c.max_temp_c {
            spans.push(ui::span(
                format!("  {t:.0}°C"),
                Style::fg(Color::usage(t.min(100.0))),
            ));
        }
        lines.push(Line::new(spans));
    }

    if let Some(p) = &st.latest.processes {
        let busiest = p
            .processes
            .first()
            .map(|t| format!("{} {:.0}%", ui::truncate(&t.name, 12), t.cpu_pct))
            .unwrap_or_default();
        lines.push(Line::new(vec![
            ui::span(format!("{:<w$}", "procs", w = LABEL_W), Style::dim()),
            ui::raw(p.total.to_string()),
            ui::span("  top ", Style::dim()),
            ui::raw(busiest),
        ]));
    }

    if let Some(m) = &st.latest.memory {
        if m.swap_total_bytes > 0 {
            lines.push(Line::new(vec![
                ui::span(format!("{:<w$}", "swap", w = LABEL_W), Style::dim()),
                ui::span(
                    format!(
                        "{} / {}",
                        ui::fmt_bytes(m.swap_used_bytes),
                        ui::fmt_bytes(m.swap_total_bytes)
                    ),
                    Style::fg(Color::usage(m.swap_used_pct)),
                ),
            ]));
        }
    }

    // A degraded verdict is worth a line even here; the border colour alone
    // is easy to miss on a dense dashboard.
    let findings = health::assess(
        st.latest.cpu.as_ref(),
        st.latest.memory.as_ref(),
        st.latest.disk.as_ref(),
    );
    if let Some(worst) = findings.first() {
        lines.push(Line::blank());
        lines.push(Line::new(vec![
            ui::span("! ", Style::fg(level_color(worst.level)).bold()),
            ui::span(
                worst.subsystem.to_string(),
                Style::fg(level_color(worst.level)),
            ),
            ui::raw(format!(" {}", worst.detail)),
        ]));
    }

    Widget::paragraph(lines).block(titled(st, "system observatory"))
}

// ── system_health ─────────────────────────────────────────────────────────

fn health_panel(st: &State) -> Widget {
    if st.latest.cpu.is_none() && st.latest.memory.is_none() {
        if let Some(e) = &st.latest.error {
            return offline(e);
        }
    }
    let findings = health::assess(
        st.latest.cpu.as_ref(),
        st.latest.memory.as_ref(),
        st.latest.disk.as_ref(),
    );
    let level = health::overall(&findings);

    let mut lines = vec![Line::new(vec![
        ui::span("● ", Style::fg(level_color(level)).bold()),
        ui::span(
            level.label().to_uppercase(),
            Style::fg(level_color(level)).bold(),
        ),
    ])];

    if findings.is_empty() {
        lines.push(Line::blank());
        lines.push(Line::text(
            "all monitored subsystems within thresholds",
            Style::dim(),
        ));
    } else {
        lines.push(Line::blank());
        for f in findings.iter().take(8) {
            lines.push(Line::new(vec![
                ui::span(
                    match f.level {
                        Level::Critical => "crit ",
                        Level::Warn => "warn ",
                        Level::Ok => "ok   ",
                    },
                    Style::fg(level_color(f.level)).bold(),
                ),
                ui::span(format!("{:<8}", f.subsystem), Style::dim()),
                ui::raw(f.detail.clone()),
            ]));
        }
    }

    // Surface the gaps: a health panel that silently ignores what it cannot
    // see is misleading. Wrapped, because this line is longer than a narrow
    // dashboard cell and clipping it would hide the point.
    if let Some(caps) = &st.caps {
        if !caps.unavailable.is_empty() {
            lines.push(Line::blank());
            lines.push(Line::text(
                format!("unmonitored: {}", gap_list(caps)),
                Style::dim(),
            ));
        }
    }

    Widget::Paragraph {
        lines,
        block: Some(titled(st, "health")),
        wrap: true,
    }
}

/// Compact gap list: `network.interfaces` + `network.connections` collapse to
/// `network.{interfaces,connections}` so the line fits a narrow panel.
fn gap_list(caps: &vanta_ext_sdk::telemetry::Capabilities) -> String {
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for g in &caps.unavailable {
        let (head, tail) = match g.topic.split_once('.') {
            Some((h, t)) => (h.to_string(), Some(t.to_string())),
            None => (g.topic.clone(), None),
        };
        match groups.iter_mut().find(|(h, _)| *h == head) {
            Some((_, subs)) => subs.extend(tail),
            None => groups.push((head, tail.into_iter().collect())),
        }
    }
    groups
        .into_iter()
        .map(|(head, subs)| match subs.len() {
            0 => head,
            1 => format!("{head}.{}", subs[0]),
            _ => format!("{head}.{{{}}}", subs.join(",")),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

// ── resource_timeline ─────────────────────────────────────────────────────

/// One timeline row: label, series, scale ceiling, formatted value, colour.
type TimelineRow = (&'static str, Vec<f64>, Option<f64>, String, Color);

fn timeline(st: &State) -> Widget {
    if st.cpu_h.is_empty() {
        return match &st.latest.error {
            Some(e) => offline(e),
            None => Widget::paragraph(vec![Line::text("collecting…", Style::dim())])
                .block(titled(st, "resource timeline")),
        };
    }
    let span_secs = st.cpu_h.len().min(WIDE_W);
    let net_peak = st.rx_h.max().max(st.tx_h.max()).max(1.0);

    let rows: Vec<TimelineRow> = vec![
        (
            "cpu",
            st.cpu_h.recent(WIDE_W),
            Some(100.0),
            format!("{:>3.0}%", st.cpu_h.last().unwrap_or(0.0)),
            Color::usage(st.cpu_h.last().unwrap_or(0.0)),
        ),
        (
            "mem",
            st.mem_h.recent(WIDE_W),
            Some(100.0),
            format!("{:>3.0}%", st.mem_h.last().unwrap_or(0.0)),
            Color::usage(st.mem_h.last().unwrap_or(0.0)),
        ),
        (
            "disk",
            st.disk_h.recent(WIDE_W),
            Some(100.0),
            format!("{:>3.0}%", st.disk_h.last().unwrap_or(0.0)),
            Color::usage(st.disk_h.last().unwrap_or(0.0)),
        ),
        (
            "rx",
            st.rx_h.recent(WIDE_W),
            Some(net_peak),
            ui::fmt_kbps(st.rx_h.last().unwrap_or(0.0)),
            Color::CYAN,
        ),
        (
            "tx",
            st.tx_h.recent(WIDE_W),
            Some(net_peak),
            ui::fmt_kbps(st.tx_h.last().unwrap_or(0.0)),
            Color::BLUE,
        ),
    ];

    let mut lines: Vec<Line> = rows
        .into_iter()
        .map(|(label, series, max, value, color)| {
            ui::metric_row(label, LABEL_W, &series, WIDE_W, max, &value, color)
        })
        .collect();

    lines.push(Line::new(vec![
        ui::span(format!("{:<w$}", "", w = LABEL_W), Style::dim()),
        ui::span(
            format!(
                "{:<w$}",
                format!("-{span_secs}s"),
                w = WIDE_W.saturating_sub(3)
            ),
            Style::dim(),
        ),
        ui::span("now", Style::dim()),
    ]));

    Widget::paragraph(lines).block(titled(st, "resource timeline"))
}

// ── resource_flow ─────────────────────────────────────────────────────────

/// Braille packs two samples per cell, so this shows twice the time span of
/// the block sparklines in the same width — the "flow" view.
fn flow(st: &State) -> Widget {
    if st.cpu_h.is_empty() {
        return match &st.latest.error {
            Some(e) => offline(e),
            None => Widget::paragraph(vec![Line::text("collecting…", Style::dim())])
                .block(titled(st, "resource flow")),
        };
    }
    let net_peak = st.rx_h.max().max(st.tx_h.max()).max(1.0);
    let rows = [
        (
            "cpu",
            st.cpu_h.recent(WIDE_W * 2),
            Some(100.0),
            Color::GREEN,
        ),
        (
            "mem",
            st.mem_h.recent(WIDE_W * 2),
            Some(100.0),
            Color::MAGENTA,
        ),
        (
            "rx",
            st.rx_h.recent(WIDE_W * 2),
            Some(net_peak),
            Color::CYAN,
        ),
        (
            "tx",
            st.tx_h.recent(WIDE_W * 2),
            Some(net_peak),
            Color::BLUE,
        ),
    ];
    let lines = rows
        .into_iter()
        .map(|(label, series, max, color)| {
            Line::new(vec![
                ui::span(format!("{:<w$}", label, w = LABEL_W), Style::dim()),
                ui::span(ui::braille_line(&series, WIDE_W, max), Style::fg(color)),
            ])
        })
        .collect();
    Widget::paragraph(lines).block(titled(st, "resource flow"))
}

// ── load_history ──────────────────────────────────────────────────────────

fn load_history(st: &State) -> Widget {
    let Some(c) = &st.latest.cpu else {
        return match &st.latest.error {
            Some(e) => offline(e),
            None => Widget::paragraph(vec![Line::text("collecting…", Style::dim())])
                .block(titled(st, "load")),
        };
    };
    let cores = c.core_count.max(1) as f64;
    let per_core = c.load1 / cores;
    let color = Color::usage(per_core * 100.0);

    // 100 on this graph = one process per core, the saturation point.
    let series = st.load_h.recent(WIDE_W);
    let ceiling = series.iter().copied().fold(100.0_f64, f64::max);

    let lines = vec![
        Line::new(vec![
            ui::span(format!("{:>5.2}", c.load1), Style::fg(color.clone()).bold()),
            ui::span(" 1m   ", Style::dim()),
            ui::span(format!("{:>5.2}", c.load5), Style::default()),
            ui::span(" 5m   ", Style::dim()),
            ui::span(format!("{:>5.2}", c.load15), Style::default()),
            ui::span(" 15m", Style::dim()),
        ]),
        Line::new(vec![
            ui::span(
                format!("{:>5.0}%", per_core * 100.0),
                Style::fg(color.clone()),
            ),
            ui::span(format!(" per core, {} cores", c.core_count), Style::dim()),
        ]),
        Line::blank(),
        Line::new(vec![ui::span(
            ui::sparkline(&series, WIDE_W, Some(ceiling)),
            Style::fg(color),
        )]),
        Line::new(vec![
            ui::span(
                format!("{:<w$}", "saturation = 100%", w = WIDE_W.saturating_sub(3)),
                Style::dim(),
            ),
            ui::span("now", Style::dim()),
        ]),
    ];

    Widget::paragraph(lines).block(titled(st, "load"))
}

/// Keep the error type referenced in this crate's public surface so a change
/// to the SDK's error contract is a compile error here rather than a silent
/// behaviour change in the panels above.
#[allow(dead_code)]
fn _assert_error_display(e: TelemetryError) -> String {
    e.to_string()
}
