//! UI construction for the Vanta protocol.
//!
//! The host protocol only has Paragraph, Gauge, List, Row and Column (see
//! `vanta/src/protocol.rs`). There is no sparkline, chart or table primitive,
//! so everything denser than a gauge is composed here from styled text spans.
//!
//! Two constraints shape this module:
//!
//! * **A widget cannot query its own area.** Nothing here may assume a width;
//!   callers pass explicit widths and the host wraps or clips.
//! * **Span budget.** The host drops any tree over 2000 spans / 100 000 chars.
//!   Builders therefore merge adjacent same-styled text instead of emitting a
//!   span per character.

use serde::Serialize;

// ── Colour / style ────────────────────────────────────────────────────────

/// Protocol colour. `Hex` serialises untagged, matching `UiColor::Hex`.
#[derive(Clone, Debug, PartialEq)]
pub enum Color {
    Named(&'static str),
    Rgb(u8, u8, u8),
    Hex(String),
}

impl Color {
    pub const RESET: Color = Color::Named("reset");
    pub const RED: Color = Color::Named("red");
    pub const GREEN: Color = Color::Named("green");
    pub const YELLOW: Color = Color::Named("yellow");
    pub const BLUE: Color = Color::Named("blue");
    pub const MAGENTA: Color = Color::Named("magenta");
    pub const CYAN: Color = Color::Named("cyan");
    pub const GRAY: Color = Color::Named("gray");
    pub const DARK_GRAY: Color = Color::Named("dark_gray");
    pub const WHITE: Color = Color::Named("white");
    pub const LIGHT_RED: Color = Color::Named("light_red");
    pub const LIGHT_GREEN: Color = Color::Named("light_green");
    pub const LIGHT_YELLOW: Color = Color::Named("light_yellow");
    pub const LIGHT_BLUE: Color = Color::Named("light_blue");
    pub const LIGHT_CYAN: Color = Color::Named("light_cyan");

    /// Severity ramp shared by every extension so colour means the same thing
    /// everywhere: calm under 60%, warning under 85%, critical above.
    pub fn usage(pct: f64) -> Color {
        if pct >= 85.0 {
            Color::RED
        } else if pct >= 60.0 {
            Color::YELLOW
        } else {
            Color::GREEN
        }
    }
}

impl Serialize for Color {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Color::Named(n) => s.serialize_str(n),
            Color::Hex(h) => s.serialize_str(h),
            Color::Rgb(r, g, b) => {
                use serde::ser::SerializeMap;
                let mut m = s.serialize_map(Some(1))?;
                m.serialize_entry("rgb", &(r, g, b))?;
                m.end()
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Style {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fg: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bg: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub underlined: Option<bool>,
}

impl Style {
    pub fn fg(c: Color) -> Self {
        Style {
            fg: Some(c),
            ..Default::default()
        }
    }
    pub fn bold(mut self) -> Self {
        self.bold = Some(true);
        self
    }
    pub fn dim() -> Self {
        Style::fg(Color::DARK_GRAY)
    }
}

// ── Spans and lines ───────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize)]
pub struct Span {
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<Style>,
}

pub fn span(content: impl Into<String>, style: Style) -> Span {
    Span {
        content: content.into(),
        style: Some(style),
    }
}

pub fn raw(content: impl Into<String>) -> Span {
    Span {
        content: content.into(),
        style: None,
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Line {
    pub spans: Vec<Span>,
}

impl Line {
    pub fn new(spans: Vec<Span>) -> Self {
        Line { spans }
    }
    pub fn text(content: impl Into<String>, style: Style) -> Self {
        Line::new(vec![span(content, style)])
    }
    pub fn blank() -> Self {
        Line::default()
    }
    pub fn width(&self) -> usize {
        self.spans.iter().map(|s| s.content.chars().count()).sum()
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Block {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub bordered: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_color: Option<Color>,
}

impl Block {
    pub fn titled(title: impl Into<String>) -> Self {
        Block {
            title: Some(title.into()),
            bordered: true,
            border_color: None,
        }
    }
    pub fn color(mut self, c: Color) -> Self {
        self.border_color = Some(c);
        self
    }
    pub fn bare() -> Self {
        Block {
            title: None,
            bordered: false,
            border_color: None,
        }
    }
}

// ── Widgets ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type")]
pub enum Widget {
    Paragraph {
        lines: Vec<Line>,
        #[serde(skip_serializing_if = "Option::is_none")]
        block: Option<Block>,
        wrap: bool,
    },
    Gauge {
        ratio: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        block: Option<Block>,
        #[serde(skip_serializing_if = "Option::is_none")]
        color: Option<Color>,
    },
    List {
        items: Vec<Line>,
        #[serde(skip_serializing_if = "Option::is_none")]
        block: Option<Block>,
    },
    Column {
        children: Vec<Widget>,
        #[serde(skip_serializing_if = "Option::is_none")]
        percentages: Option<Vec<u16>>,
    },
    Row {
        children: Vec<Widget>,
        #[serde(skip_serializing_if = "Option::is_none")]
        percentages: Option<Vec<u16>>,
    },
}

impl Widget {
    pub fn paragraph(lines: Vec<Line>) -> Widget {
        Widget::Paragraph {
            lines,
            block: None,
            wrap: false,
        }
    }
    pub fn list(items: Vec<Line>) -> Widget {
        Widget::List { items, block: None }
    }
    /// Stack children vertically.
    pub fn column(children: Vec<Widget>) -> Widget {
        Widget::Column {
            children,
            percentages: None,
        }
    }
    /// Place children side by side.
    pub fn row(children: Vec<Widget>) -> Widget {
        Widget::Row {
            children,
            percentages: None,
        }
    }
    /// Attach a bordered/titled block. No-op for layout widgets, which the
    /// protocol does not allow to carry a block.
    pub fn block(self, b: Block) -> Widget {
        match self {
            Widget::Paragraph { lines, wrap, .. } => Widget::Paragraph {
                lines,
                block: Some(b),
                wrap,
            },
            Widget::List { items, .. } => Widget::List {
                items,
                block: Some(b),
            },
            Widget::Gauge {
                ratio,
                label,
                color,
                ..
            } => Widget::Gauge {
                ratio,
                label,
                block: Some(b),
                color,
            },
            other => other,
        }
    }
    pub fn percentages(self, p: Vec<u16>) -> Widget {
        match self {
            Widget::Column { children, .. } => Widget::Column {
                children,
                percentages: Some(p),
            },
            Widget::Row { children, .. } => Widget::Row {
                children,
                percentages: Some(p),
            },
            other => other,
        }
    }
    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_else(|_| b"{}".to_vec())
    }
}

// ── Primitives ────────────────────────────────────────────────────────────

/// Eighth-block ramp, used for both horizontal meters and vertical graphs.
const BLOCKS_V: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
const BLOCKS_H: [char; 9] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];

/// Single-row sparkline. Values are scaled to `max` (or the series peak when
/// `max` is None), so a flat series does not render as full height.
pub fn sparkline(values: &[f64], width: usize, max: Option<f64>) -> String {
    if width == 0 {
        return String::new();
    }
    let peak = max
        .unwrap_or_else(|| values.iter().copied().fold(0.0_f64, f64::max))
        .max(f64::EPSILON);
    let take = values.len().min(width);
    let start = values.len() - take;
    let mut out = String::with_capacity(width);
    // Right-align: newest sample at the right edge.
    for _ in 0..width - take {
        out.push(' ');
    }
    for v in &values[start..] {
        let t = (v / peak).clamp(0.0, 1.0);
        out.push(BLOCKS_V[(t * 8.0).round() as usize]);
    }
    out
}

/// Horizontal meter with eighth-cell resolution.
pub fn bar(ratio: f64, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let levels = width * 8;
    let filled = (ratio.clamp(0.0, 1.0) * levels as f64).round() as usize;
    (0..width)
        .map(|i| BLOCKS_H[filled.saturating_sub(i * 8).min(8)])
        .collect()
}

/// Filled / unfilled halves of a track, for two-colour meters.
pub fn track(ratio: f64, width: usize) -> (String, String) {
    let filled = ((ratio.clamp(0.0, 1.0)) * width as f64).round() as usize;
    let filled = filled.min(width);
    ("━".repeat(filled), "─".repeat(width - filled))
}

/// Braille sparkline: 2 samples per cell, so twice the horizontal resolution
/// of `sparkline` at the cost of a dotted look.
pub fn braille_line(values: &[f64], width: usize, max: Option<f64>) -> String {
    if width == 0 {
        return String::new();
    }
    let peak = max
        .unwrap_or_else(|| values.iter().copied().fold(0.0_f64, f64::max))
        .max(f64::EPSILON);
    let cells = width;
    let need = cells * 2;
    let take = values.len().min(need);
    let start = values.len() - take;
    let vals = &values[start..];
    let mut out = String::with_capacity(cells);
    for _ in 0..(need - take) / 2 {
        out.push(' ');
    }
    // Braille dot rows: bits 0..3 = left column top→bottom, 3..7 = right.
    const LEFT: [u8; 4] = [0x01, 0x02, 0x04, 0x40];
    const RIGHT: [u8; 4] = [0x08, 0x10, 0x20, 0x80];
    for pair in vals.chunks(2) {
        let mut bits = 0u8;
        for (i, v) in pair.iter().enumerate() {
            let t = (v / peak).clamp(0.0, 1.0);
            let level = (t * 4.0).ceil() as usize; // 0..4 dots lit
            let col = if i == 0 { &LEFT } else { &RIGHT };
            for row in 0..level.min(4) {
                bits |= col[3 - row];
            }
        }
        out.push(char::from_u32(0x2800 + bits as u32).unwrap_or(' '));
    }
    out
}

/// `label  ▁▂▃▅▇  value` — the workhorse row for dense metric panels.
pub fn metric_row(
    label: &str,
    label_w: usize,
    series: &[f64],
    spark_w: usize,
    max: Option<f64>,
    value: &str,
    color: Color,
) -> Line {
    Line::new(vec![
        span(
            format!("{:<w$}", truncate(label, label_w), w = label_w),
            Style::dim(),
        ),
        span(sparkline(series, spark_w, max), Style::fg(color.clone())),
        raw(" "),
        span(value.to_string(), Style::fg(color).bold()),
    ])
}

/// `● label   value` status line.
pub fn status_row(ok: bool, label: &str, label_w: usize, value: &str) -> Line {
    let (dot, c) = if ok {
        ("●", Color::GREEN)
    } else {
        ("●", Color::RED)
    };
    Line::new(vec![
        span(dot, Style::fg(c)),
        raw(" "),
        span(
            format!("{:<w$}", truncate(label, label_w), w = label_w),
            Style::dim(),
        ),
        raw(value.to_string()),
    ])
}

/// Fixed-width table. Columns are `(header, width, right_aligned)`; cells are
/// clipped to their column so a long value cannot break the layout.
pub struct Table {
    cols: Vec<(String, usize, bool)>,
    rows: Vec<Vec<(String, Style)>>,
}

impl Table {
    pub fn new(cols: &[(&str, usize, bool)]) -> Self {
        Table {
            cols: cols
                .iter()
                .map(|(h, w, r)| (h.to_string(), *w, *r))
                .collect(),
            rows: Vec::new(),
        }
    }

    pub fn row(&mut self, cells: Vec<(String, Style)>) -> &mut Self {
        self.rows.push(cells);
        self
    }

    pub fn header(&self) -> Line {
        let spans = self
            .cols
            .iter()
            .map(|(h, w, right)| span(pad(h, *w, *right), Style::dim()))
            .collect::<Vec<_>>();
        Line::new(interleave(spans))
    }

    pub fn lines(&self) -> Vec<Line> {
        let mut out = Vec::with_capacity(self.rows.len() + 1);
        out.push(self.header());
        for row in &self.rows {
            let spans = row
                .iter()
                .zip(&self.cols)
                .map(|((text, style), (_, w, right))| span(pad(text, *w, *right), style.clone()))
                .collect::<Vec<_>>();
            out.push(Line::new(interleave(spans)));
        }
        out
    }
}

fn interleave(spans: Vec<Span>) -> Vec<Span> {
    let mut out = Vec::with_capacity(spans.len() * 2);
    for (i, s) in spans.into_iter().enumerate() {
        if i > 0 {
            out.push(raw(" "));
        }
        out.push(s);
    }
    out
}

fn pad(s: &str, w: usize, right: bool) -> String {
    let t = truncate(s, w);
    let fill = w.saturating_sub(t.chars().count());
    if right {
        format!("{}{}", " ".repeat(fill), t)
    } else {
        format!("{}{}", t, " ".repeat(fill))
    }
}

/// Char-safe truncation with an ellipsis. Byte slicing would panic on the
/// non-ASCII text that turns up in process names and note titles.
pub fn truncate(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out.push('…');
    out
}

pub fn fmt_bytes(b: u64) -> String {
    const GIB: f64 = 1_073_741_824.0;
    const MIB: f64 = 1_048_576.0;
    const KIB: f64 = 1024.0;
    let f = b as f64;
    if f >= GIB {
        format!("{:.1}G", f / GIB)
    } else if f >= MIB {
        format!("{:.0}M", f / MIB)
    } else if f >= KIB {
        format!("{:.0}K", f / KIB)
    } else {
        format!("{b}B")
    }
}

pub fn fmt_kbps(kbps: f64) -> String {
    if kbps >= 1024.0 {
        format!("{:.1}M/s", kbps / 1024.0)
    } else if kbps >= 1.0 {
        format!("{:.0}K/s", kbps)
    } else {
        format!("{:.0}B/s", kbps * 1024.0)
    }
}

/// Standard "this data does not exist" panel. Extensions must render this
/// rather than inventing numbers when the host cannot supply a topic.
pub fn unavailable(title: &str, reason: &str) -> Widget {
    Widget::Paragraph {
        lines: vec![
            Line::text("data unavailable", Style::fg(Color::YELLOW).bold()),
            Line::text(reason.to_string(), Style::dim()),
        ],
        block: Some(Block::titled(format!(" {title} ")).color(Color::DARK_GRAY)),
        wrap: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparkline_is_right_aligned_and_scaled() {
        assert_eq!(sparkline(&[], 4, None), "    ");
        // Fewer samples than width: padded on the left, newest at the right.
        assert_eq!(sparkline(&[100.0], 3, Some(100.0)), "  █");
        assert_eq!(sparkline(&[0.0, 50.0, 100.0], 3, Some(100.0)), " ▄█");
        // Auto-scaled to the series peak.
        assert_eq!(sparkline(&[5.0, 10.0], 2, None), "▄█");
        assert_eq!(sparkline(&[0.0, 0.0], 2, None).chars().count(), 2);
    }

    #[test]
    fn sparkline_clamps_out_of_range_values() {
        let s = sparkline(&[-5.0, 500.0], 2, Some(100.0));
        assert_eq!(s, " █");
    }

    #[test]
    fn bar_has_eighth_resolution() {
        assert_eq!(bar(0.0, 4), "    ");
        assert_eq!(bar(1.0, 4), "████");
        assert_eq!(bar(0.5, 4), "██  ");
        assert_eq!(bar(2.0, 2), "██");
    }

    #[test]
    fn braille_line_packs_two_samples_per_cell() {
        assert_eq!(
            braille_line(&[1.0, 1.0, 1.0, 1.0], 2, Some(1.0))
                .chars()
                .count(),
            2
        );
        assert!(braille_line(&[1.0; 8], 4, Some(1.0))
            .chars()
            .all(|c| c as u32 >= 0x2800));
    }

    #[test]
    fn truncate_is_char_safe() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 6), "hello…");
        // Multi-byte input must not panic or split a codepoint.
        assert_eq!(truncate("日本語のプロセス", 4), "日本語…");
        assert_eq!(truncate("émile—x", 3), "ém…");
        assert_eq!(truncate("abc", 0), "");
    }

    #[test]
    fn table_pads_and_clips_to_column_width() {
        let mut t = Table::new(&[("pid", 5, true), ("name", 6, false)]);
        t.row(vec![
            ("1234".into(), Style::default()),
            ("averylongname".into(), Style::default()),
        ]);
        let lines = t.lines();
        assert_eq!(lines.len(), 2);
        // header + row are the same width: 5 + 1 separator + 6
        assert_eq!(lines[0].width(), 12);
        assert_eq!(lines[1].width(), 12);
        let row: String = lines[1].spans.iter().map(|s| s.content.clone()).collect();
        assert_eq!(row, " 1234 avery…");
    }

    #[test]
    fn colors_serialize_in_protocol_shape() {
        assert_eq!(serde_json::to_string(&Color::RED).unwrap(), "\"red\"");
        assert_eq!(
            serde_json::to_string(&Color::Hex("#ff8800".into())).unwrap(),
            "\"#ff8800\""
        );
        assert_eq!(
            serde_json::to_string(&Color::Rgb(1, 2, 3)).unwrap(),
            "{\"rgb\":[1,2,3]}"
        );
    }

    #[test]
    fn usage_ramp_matches_host_conventions() {
        assert_eq!(Color::usage(10.0), Color::GREEN);
        assert_eq!(Color::usage(70.0), Color::YELLOW);
        assert_eq!(Color::usage(99.0), Color::RED);
    }

    #[test]
    fn widget_json_matches_host_protocol_tags() {
        let w = Widget::paragraph(vec![Line::text("hi", Style::fg(Color::RED))])
            .block(Block::titled(" t "));
        let v: serde_json::Value = serde_json::from_slice(&w.to_json()).unwrap();
        assert_eq!(v["type"], "Paragraph");
        assert_eq!(v["block"]["title"], " t ");
        assert_eq!(v["block"]["bordered"], true);
        assert_eq!(v["lines"][0]["spans"][0]["style"]["fg"], "red");
        assert_eq!(v["wrap"], false);

        let r = Widget::row(vec![w]).percentages(vec![100]);
        let v: serde_json::Value = serde_json::from_slice(&r.to_json()).unwrap();
        assert_eq!(v["type"], "Row");
        assert_eq!(v["percentages"][0], 100);
    }

    #[test]
    fn formatting_helpers() {
        assert_eq!(fmt_bytes(512), "512B");
        assert_eq!(fmt_bytes(2048), "2K");
        assert_eq!(fmt_bytes(5 * 1_048_576), "5M");
        assert_eq!(fmt_bytes(3 * 1_073_741_824), "3.0G");
        assert_eq!(fmt_kbps(0.5), "512B/s");
        assert_eq!(fmt_kbps(20.0), "20K/s");
        assert_eq!(fmt_kbps(2048.0), "2.0M/s");
    }
}
