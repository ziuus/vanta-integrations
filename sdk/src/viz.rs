//! Terminal-native visualization primitives.
//!
//! These exist because the host protocol has no chart widget — only styled
//! text. Each one answers a specific question densely; none is decorative.
//! All are pure functions over plain data so they can be unit-tested without
//! a host, and none allocates per character.

use crate::ui::{raw, span, Color, Line, Span, Style};

// ── State machine ─────────────────────────────────────────────────────────

/// Horizontal state machine: `norm ─▶ detect ─▶ OPEN ─▶ persist ─▶ recov`.
///
/// The active stage is bold and coloured, stages already passed are plain,
/// stages not reached are dim — so the current position and the direction of
/// travel are both readable at a glance.
pub fn state_machine(stages: &[&str], active: usize, color: Color) -> Line {
    let mut spans = Vec::with_capacity(stages.len() * 2);
    for (i, s) in stages.iter().enumerate() {
        if i > 0 {
            let passed = i <= active;
            spans.push(span(
                "─▶",
                if passed {
                    Style::fg(color.clone())
                } else {
                    Style::dim()
                },
            ));
        }
        let style = match i.cmp(&active) {
            std::cmp::Ordering::Equal => Style::fg(color.clone()).bold(),
            std::cmp::Ordering::Less => Style::fg(Color::GRAY),
            std::cmp::Ordering::Greater => Style::dim(),
        };
        let label = if i == active {
            format!("[{}]", s.to_uppercase())
        } else {
            format!(" {s} ")
        };
        spans.push(span(label, style));
    }
    Line::new(spans)
}

// ── Threshold gauge ───────────────────────────────────────────────────────

/// A value shown against its clear and trigger thresholds.
///
/// Unlike a plain usage bar this is drawn in the detection model's own terms:
/// the band below `clear` is recovery, the band between `clear` and `trigger`
/// is hysteresis, and above `trigger` is breach. Markers `╵` sit at the two
/// thresholds so their position is explicit rather than implied by colour.
///
/// ```text
/// ▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒░░░░░░░░  ← fill to value
///           ╵     ╵
///        clear  trigger
/// ```
pub fn threshold_gauge(
    value: f64,
    clear: f64,
    trigger: f64,
    scale_max: f64,
    width: usize,
    color: Color,
) -> Vec<Line> {
    if width < 4 {
        return Vec::new();
    }
    let max = scale_max.max(trigger * 1.05).max(value).max(f64::EPSILON);
    let pos = |v: f64| ((v / max).clamp(0.0, 1.0) * width as f64).round() as usize;
    let (v_cell, c_cell, t_cell) = (pos(value).min(width), pos(clear), pos(trigger));

    // Bar: filled to the value, with the hysteresis and breach zones shaded
    // differently so the zones remain visible past the fill.
    let mut bar: Vec<Span> = Vec::with_capacity(3);
    let mut cur_style = 0u8;
    let mut buf = String::new();
    let flush = |bar: &mut Vec<Span>, buf: &mut String, kind: u8, color: &Color| {
        if buf.is_empty() {
            return;
        }
        let st = match kind {
            0 => Style::fg(color.clone()),    // filled
            1 => Style::fg(Color::DARK_GRAY), // unfilled, below clear
            _ => Style::fg(Color::GRAY),      // unfilled, above clear
        };
        bar.push(span(std::mem::take(buf), st));
    };
    for i in 0..width {
        let (ch, kind) = if i < v_cell {
            ('▓', 0u8)
        } else if i < c_cell {
            ('░', 1)
        } else {
            ('░', 2)
        };
        if kind != cur_style && !buf.is_empty() {
            flush(&mut bar, &mut buf, cur_style, &color);
        }
        cur_style = kind;
        buf.push(ch);
    }
    flush(&mut bar, &mut buf, cur_style, &color);

    // Marker row: one tick per threshold.
    let mut ticks = vec![' '; width];
    if c_cell < width {
        ticks[c_cell] = '╵';
    }
    if t_cell < width {
        ticks[t_cell] = '╵';
    }
    let tick_line = Line::new(vec![span(
        ticks.into_iter().collect::<String>(),
        Style::dim(),
    )]);

    vec![Line::new(bar), tick_line]
}

// ── Duration meter ────────────────────────────────────────────────────────

/// How long something has persisted, against human milestones.
///
/// A linear bar is useless here because incidents span seconds to hours, so
/// the scale is logarithmic across the supplied milestones and each reached
/// milestone lights up.
pub fn duration_meter(elapsed_ms: u64, milestones_s: &[u64], width: usize) -> Line {
    if milestones_s.is_empty() || width == 0 {
        return Line::blank();
    }
    let secs = elapsed_ms / 1000;
    let seg = (width / milestones_s.len()).max(1);
    let mut spans = Vec::new();
    for (i, m) in milestones_s.iter().enumerate() {
        let prev = if i == 0 { 0 } else { milestones_s[i - 1] };
        let reached = secs >= *m;
        let partial = !reached && secs > prev;
        let filled = if reached {
            seg
        } else if partial {
            let span_len = (m - prev).max(1);
            (((secs - prev) as f64 / span_len as f64) * seg as f64).round() as usize
        } else {
            0
        };
        let color = if reached {
            // Later milestones mean a longer-running incident: escalate.
            match i {
                0 => Color::YELLOW,
                1 => Color::YELLOW,
                _ => Color::RED,
            }
        } else {
            Color::DARK_GRAY
        };
        spans.push(span("█".repeat(filled), Style::fg(color)));
        spans.push(span("·".repeat(seg - filled), Style::dim()));
    }
    Line::new(spans)
}

// ── Event density strip ───────────────────────────────────────────────────

/// Activity over a time window, newest at the right.
///
/// Answers "how busy has Sentinel been", which a list of events cannot show
/// at a glance. Buckets are fixed-width in time so the strip is comparable
/// between renders.
pub fn density_strip(timestamps_ms: &[u64], now_ms: u64, window_ms: u64, buckets: usize) -> String {
    if buckets == 0 || window_ms == 0 {
        return String::new();
    }
    let mut counts = vec![0usize; buckets];
    let per = (window_ms / buckets as u64).max(1);
    for t in timestamps_ms {
        let age = now_ms.saturating_sub(*t);
        if age >= window_ms {
            continue;
        }
        // Newest at the right.
        let idx = buckets - 1 - (age / per).min(buckets as u64 - 1) as usize;
        counts[idx] += 1;
    }
    let peak = counts.iter().copied().max().unwrap_or(0);
    const RAMP: [char; 5] = ['·', '▁', '▃', '▆', '█'];
    counts
        .into_iter()
        .map(|c| {
            if c == 0 {
                RAMP[0]
            } else {
                let t = c as f64 / peak.max(1) as f64;
                RAMP[1 + ((t * 3.0).round() as usize).min(3)]
            }
        })
        .collect()
}

// ── Signal strip with markers ─────────────────────────────────────────────

/// A marker placed on a signal strip at a point in time.
#[derive(Clone, Debug)]
pub struct Marker {
    /// Index into the series.
    pub at: usize,
    pub glyph: char,
    pub color: Color,
}

/// The signal around an incident, with a threshold line and event markers.
///
/// Returns (signal line, marker line). Samples above `trigger` are drawn in
/// the breach colour, so the breach window is visible in the shape itself
/// rather than only in the markers.
pub fn signal_strip(
    values: &[f64],
    trigger: f64,
    width: usize,
    markers: &[Marker],
    normal: Color,
    breach: Color,
) -> (Line, Line) {
    const RAMP: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    if width == 0 {
        return (Line::blank(), Line::blank());
    }
    let take = values.len().min(width);
    let start = values.len() - take;
    let peak = values
        .iter()
        .copied()
        .fold(trigger, f64::max)
        .max(f64::EPSILON);

    let mut spans = Vec::new();
    if take < width {
        spans.push(raw(" ".repeat(width - take)));
    }
    // Merge runs of the same colour into one span to stay inside the host's
    // span budget.
    let mut buf = String::new();
    let mut cur_breach = None;
    for v in &values[start..] {
        let is_breach = *v >= trigger;
        if cur_breach != Some(is_breach) && !buf.is_empty() {
            let c = if cur_breach == Some(true) {
                breach.clone()
            } else {
                normal.clone()
            };
            spans.push(span(std::mem::take(&mut buf), Style::fg(c)));
        }
        cur_breach = Some(is_breach);
        let t = (v / peak).clamp(0.0, 1.0);
        buf.push(RAMP[(t * 8.0).round() as usize]);
    }
    if !buf.is_empty() {
        let c = if cur_breach == Some(true) {
            breach
        } else {
            normal
        };
        spans.push(span(buf, Style::fg(c)));
    }

    // Marker row aligned under the signal.
    let offset = width - take;
    let mut row: Vec<(char, Color)> = vec![(' ', Color::RESET); width];
    for m in markers {
        if m.at >= start {
            let col = offset + (m.at - start);
            if col < width {
                row[col] = (m.glyph, m.color.clone());
            }
        }
    }
    let mut mspans = Vec::new();
    let mut mbuf = String::new();
    let mut mcur: Option<Color> = None;
    for (ch, c) in row {
        let same = matches!((&mcur, &c), (Some(a), b) if a == b);
        if !same && !mbuf.is_empty() {
            mspans.push(span(
                std::mem::take(&mut mbuf),
                Style::fg(mcur.clone().unwrap_or(Color::RESET)),
            ));
        }
        mcur = Some(c);
        mbuf.push(ch);
    }
    if !mbuf.is_empty() {
        mspans.push(span(mbuf, Style::fg(mcur.unwrap_or(Color::RESET))));
    }

    (Line::new(spans), Line::new(mspans))
}

// ── Ranking bars ──────────────────────────────────────────────────────────

/// Compact ranked bars, e.g. processes by CPU at the moment of an incident.
/// Scaled to the largest entry so relative weight is what reads, not the
/// absolute number (which is printed anyway).
pub fn ranking_bars(
    entries: &[(String, f64, String)],
    label_w: usize,
    bar_w: usize,
    color: Color,
) -> Vec<Line> {
    let peak = entries
        .iter()
        .map(|(_, v, _)| *v)
        .fold(f64::EPSILON, f64::max);
    entries
        .iter()
        .map(|(label, value, display)| {
            let filled = ((value / peak).clamp(0.0, 1.0) * bar_w as f64).round() as usize;
            Line::new(vec![
                span(
                    format!("{:<w$}", crate::ui::truncate(label, label_w), w = label_w),
                    Style::default(),
                ),
                span("▉".repeat(filled), Style::fg(color.clone())),
                span("·".repeat(bar_w - filled), Style::dim()),
                span(format!(" {display}"), Style::dim()),
            ])
        })
        .collect()
}

// ── Sparkbar row ──────────────────────────────────────────────────────────

/// Coverage indicator: one glyph per monitored condition.
/// `●` available, `◐` stale, `○` unavailable.
pub fn coverage_dots(entries: &[(&str, Coverage)]) -> Line {
    let mut spans = Vec::new();
    for (i, (label, c)) in entries.iter().enumerate() {
        if i > 0 {
            spans.push(raw(" "));
        }
        let (g, col) = match c {
            Coverage::Live => ('●', Color::GREEN),
            Coverage::Stale => ('◐', Color::YELLOW),
            Coverage::Missing => ('○', Color::DARK_GRAY),
        };
        spans.push(span(g.to_string(), Style::fg(col)));
        spans.push(span(format!(" {label}"), Style::dim()));
    }
    Line::new(spans)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coverage {
    Live,
    Stale,
    Missing,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(l: &Line) -> String {
        l.spans.iter().map(|s| s.content.clone()).collect()
    }

    #[test]
    fn state_machine_marks_the_active_stage() {
        let l = state_machine(&["norm", "detect", "open"], 1, Color::YELLOW);
        let t = text(&l);
        assert!(t.contains("[DETECT]"), "{t}");
        assert!(t.contains(" norm "));
        assert!(t.contains("─▶"));
    }

    #[test]
    fn state_machine_dims_unreached_stages() {
        let l = state_machine(&["a", "b", "c"], 0, Color::GREEN);
        // First stage active, rest dim.
        assert_eq!(l.spans[0].content, "[A]");
        let dim = l
            .spans
            .iter()
            .filter(|s| matches!(&s.style, Some(st) if st.fg == Some(Color::DARK_GRAY)));
        assert!(dim.count() >= 2);
    }

    #[test]
    fn threshold_gauge_places_ticks_at_thresholds() {
        let lines = threshold_gauge(90.0, 75.0, 85.0, 100.0, 20, Color::RED);
        assert_eq!(lines.len(), 2);
        let bar = text(&lines[0]);
        let ticks = text(&lines[1]);
        assert_eq!(bar.chars().count(), 20);
        assert_eq!(ticks.chars().count(), 20);
        assert_eq!(ticks.matches('╵').count(), 2);
        // clear tick left of trigger tick
        assert!(ticks.find('╵').unwrap() < ticks.rfind('╵').unwrap());
        // filled to 90% of scale
        assert_eq!(bar.matches('▓').count(), 18);
    }

    #[test]
    fn threshold_gauge_handles_value_over_scale() {
        let lines = threshold_gauge(500.0, 75.0, 85.0, 100.0, 10, Color::RED);
        assert_eq!(text(&lines[0]).chars().count(), 10);
    }

    #[test]
    fn duration_meter_lights_reached_milestones() {
        // 0s: nothing lit.
        let l = duration_meter(0, &[10, 60, 300], 12);
        assert_eq!(text(&l).matches('█').count(), 0);
        // 70s: first two milestones fully lit.
        let l = duration_meter(70_000, &[10, 60, 300], 12);
        let s = text(&l);
        assert!(s.matches('█').count() >= 8, "{s}");
        // Beyond the last milestone: fully lit.
        let l = duration_meter(9_999_000, &[10, 60, 300], 12);
        assert_eq!(text(&l).matches('█').count(), 12);
    }

    #[test]
    fn density_strip_puts_newest_on_the_right() {
        let now = 100_000;
        // One event just now, none earlier.
        let s = density_strip(&[99_000], now, 60_000, 10);
        assert_eq!(s.chars().count(), 10);
        assert_eq!(s.chars().next_back().unwrap(), '█');
        assert_eq!(s.chars().next().unwrap(), '·');
    }

    #[test]
    fn density_strip_ignores_events_outside_the_window() {
        let s = density_strip(&[1_000], 500_000, 60_000, 8);
        assert!(s.chars().all(|c| c == '·'));
        assert!(density_strip(&[], 0, 1000, 0).is_empty());
    }

    #[test]
    fn signal_strip_colours_breaching_samples_differently() {
        let vals = [10.0, 20.0, 95.0, 96.0];
        let (sig, marks) = signal_strip(&vals, 85.0, 4, &[], Color::GREEN, Color::RED);
        assert_eq!(text(&sig).chars().count(), 4);
        assert_eq!(text(&marks).chars().count(), 4);
        // Two runs: normal then breach.
        let breach_spans = sig
            .spans
            .iter()
            .filter(|s| matches!(&s.style, Some(st) if st.fg == Some(Color::RED)))
            .count();
        assert_eq!(breach_spans, 1);
    }

    #[test]
    fn signal_strip_places_markers_under_their_sample() {
        let vals = [1.0, 2.0, 3.0, 4.0];
        let m = [Marker {
            at: 3,
            glyph: '▲',
            color: Color::RED,
        }];
        let (_, marks) = signal_strip(&vals, 99.0, 4, &m, Color::GREEN, Color::RED);
        let t = text(&marks);
        assert_eq!(t.chars().nth(3), Some('▲'));
        assert_eq!(t.chars().filter(|c| *c == '▲').count(), 1);
    }

    #[test]
    fn signal_strip_right_aligns_a_short_series() {
        let (sig, _) = signal_strip(&[5.0], 10.0, 6, &[], Color::GREEN, Color::RED);
        let t = text(&sig);
        assert_eq!(t.chars().count(), 6);
        assert!(t.starts_with("     "));
    }

    #[test]
    fn ranking_bars_scale_to_the_largest_entry() {
        let e = vec![
            ("hog".to_string(), 100.0, "100%".to_string()),
            ("small".to_string(), 25.0, "25%".to_string()),
        ];
        let lines = ranking_bars(&e, 8, 8, Color::RED);
        assert_eq!(lines.len(), 2);
        let first: String = text(&lines[0]);
        let second: String = text(&lines[1]);
        assert_eq!(first.matches('▉').count(), 8);
        assert_eq!(second.matches('▉').count(), 2);
    }

    #[test]
    fn coverage_dots_distinguish_live_stale_and_missing() {
        let l = coverage_dots(&[
            ("cpu", Coverage::Live),
            ("net", Coverage::Stale),
            ("containers", Coverage::Missing),
        ]);
        let t = text(&l);
        assert!(t.contains("● cpu"));
        assert!(t.contains("◐ net"));
        assert!(t.contains("○ containers"));
    }
}
