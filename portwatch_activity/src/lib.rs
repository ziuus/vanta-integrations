//! PortWatch — what network ports is this machine exposing, and which process
//! owns them?
//!
//! This is not a network throughput graph. It answers the operational question:
//! > "What is actually listening right now, what process started it, and has
//! >  anything opened or closed recently?"
//!
//! # Widgets
//!
//! | widget            | answers                                              |
//! |---|---|
//! | `portwatch`       | full listener table + port activity feed             |
//! | `port_listeners`  | compact listener-only list (fits small panels)       |
//! | `port_activity`   | just the open/close event feed                       |
//!
//! # Architecture
//!
//! One host call per render (topic: `connections`). The engine:
//! - Extracts LISTEN-state entries and deduplicates by (port, protocol).
//! - Diffs against the previous snapshot to detect new and closed listeners.
//! - Appends diff events to a bounded activity ring.
//! - Classifies each listener as "exposed" (any-addr) or "local-only".
//!
//! No host-budget risk: `connections` is a single topic, always < 5 ms.

use extism_pdk::*;
use std::cell::RefCell;
use std::collections::HashSet;

use vanta_ext_sdk::history::now_ms;
use vanta_ext_sdk::telemetry::{self, ConnectionsSnapshot};
use vanta_ext_sdk::ui::{self, Block, Color, Line, Style, Table, Widget};
use vanta_ext_sdk::{ExtensionMetadata, API_VERSION_TELEMETRY};

const ID: &str = "portwatch";
const VERSION: &str = "0.1.0";

/// Minimum ms between fetches.
const REFRESH_MS: u64 = 1_000;
/// Maximum activity events retained.
const ACTIVITY_RING: usize = 20;

// ── Port activity events ──────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventKind {
    Opened,
    Closed,
}

#[derive(Debug, Clone)]
pub struct PortEvent {
    pub port: u16,
    pub protocol: String,
    pub process_name: String,
    pub kind: EventKind,
    pub ts_ms: u64,
}

impl PortEvent {
    fn age_str(&self, now_ms: u64) -> String {
        let secs = now_ms.saturating_sub(self.ts_ms) / 1000;
        if secs < 60 {
            format!("{secs}s ago")
        } else {
            format!("{}m ago", secs / 60)
        }
    }
}

// ── Listener key — deduplication ─────────────────────────────────────────────

/// Canonical key for one listening socket: (port, protocol).
/// We deduplicate by port+protocol rather than pid, because the same port can
/// appear as both tcp and tcp6 (dual-stack) or across multiple interfaces.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ListenerKey {
    pub port: u16,
    pub protocol: String,
}

/// One unique listener after deduplication.
#[derive(Debug, Clone)]
pub struct Listener {
    pub port: u16,
    pub protocol: String,
    pub process_name: String,
    pub pid: Option<u32>,
    /// True if bound to any-address (0.0.0.0 or [::]).
    pub exposed: bool,
}

impl Listener {
    fn key(&self) -> ListenerKey {
        ListenerKey {
            port: self.port,
            protocol: self.protocol.clone(),
        }
    }
}

// ── Shared state ──────────────────────────────────────────────────────────────

pub struct Store {
    pub last_snap: Option<ConnectionsSnapshot>,
    /// Deduplicated listeners from the last snapshot.
    pub listeners: Vec<Listener>,
    /// Keys of listeners seen in the previous cycle — used for diffing.
    pub prev_keys: HashSet<ListenerKey>,
    /// Bounded activity event ring.
    pub activity: Vec<PortEvent>,
    pub last_fetch_ms: u64,
    pub connections_available: bool,
    pub caps_done: bool,
    pub error: Option<String>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            last_snap: None,
            listeners: Vec::new(),
            prev_keys: HashSet::new(),
            activity: Vec::new(),
            last_fetch_ms: 0,
            connections_available: true,
            caps_done: false,
            error: None,
        }
    }
}

thread_local! {
    static STORE: RefCell<Store> = RefCell::new(Store::default());
}

// ── Engine ────────────────────────────────────────────────────────────────────

/// Deduplicate LISTEN entries into a sorted listener list.
/// Sorted: exposed first, then by port ascending.
pub fn extract_listeners(snap: &ConnectionsSnapshot) -> Vec<Listener> {
    let mut seen: std::collections::HashMap<ListenerKey, Listener> =
        std::collections::HashMap::new();

    for c in snap.listeners() {
        let Some(port) = c.local_port() else {
            continue;
        };
        // Protocol: strip "6" suffix for readability ("tcp6" → "tcp") unless
        // the port only appears on tcp6. We keep the raw string here; display
        // can simplify later.
        let key = ListenerKey {
            port,
            protocol: "tcp".to_string(), // unify tcp and tcp6
        };
        let exposed = !c.is_localhost_only()
            && !c.local_addr.starts_with("127.")
            && (c.local_addr.starts_with("0.0.0.0") || c.local_addr.starts_with("[::"));

        let entry = seen.entry(key).or_insert_with(|| Listener {
            port,
            protocol: "tcp".to_string(),
            process_name: c.process_name.clone().unwrap_or_else(|| "?".to_string()),
            pid: c.pid,
            exposed,
        });
        // Prefer an attributed name over "?".
        if entry.process_name == "?" {
            if let Some(ref name) = c.process_name {
                entry.process_name = name.clone();
                entry.pid = c.pid;
            }
        }
        // Mark exposed if any addr for this port is exposed.
        if exposed {
            entry.exposed = true;
        }
    }

    let mut list: Vec<Listener> = seen.into_values().collect();
    // Exposed first, then port ascending.
    list.sort_by(|a, b| b.exposed.cmp(&a.exposed).then(a.port.cmp(&b.port)));
    list
}

fn tick() {
    let now = now_ms();

    // Capabilities check — once only. Use try_borrow so a killed-render can't
    // leave the RefCell poisoned for subsequent calls.
    let need_caps = STORE.with(|s| s.try_borrow().map(|st| !st.caps_done).unwrap_or(false));
    if need_caps {
        let caps = telemetry::capabilities();
        STORE.with(|s| {
            if let Ok(mut st) = s.try_borrow_mut() {
                st.caps_done = true;
                if let Ok(c) = caps {
                    st.connections_available = c.has("connections");
                }
            }
        });
    }

    let (due, available) = STORE.with(|s| {
        match s.try_borrow() {
            Ok(st) => (
                now.saturating_sub(st.last_fetch_ms) >= REFRESH_MS,
                st.connections_available,
            ),
            Err(_) => (false, false), // RefCell still borrowed from aborted render
        }
    });

    if !due || !available {
        return;
    }

    // Host call — no borrow held.
    let result = telemetry::connections();

    STORE.with(|s| {
        if let Ok(mut st) = s.try_borrow_mut() {
            st.last_fetch_ms = now;

            match result {
                Err(e) => st.error = Some(e.to_string()),
                Ok(snap) => {
                    st.error = None;
                    let new_listeners = extract_listeners(&snap);
                    let new_keys: HashSet<ListenerKey> =
                        new_listeners.iter().map(|l| l.key()).collect();

                    // Detect opens (in new, not in prev).
                    for l in &new_listeners {
                        if !st.prev_keys.contains(&l.key()) {
                            let ev = PortEvent {
                                port: l.port,
                                protocol: l.protocol.clone(),
                                process_name: l.process_name.clone(),
                                kind: EventKind::Opened,
                                ts_ms: now,
                            };
                            if st.activity.len() >= ACTIVITY_RING {
                                st.activity.remove(0);
                            }
                            st.activity.push(ev);
                        }
                    }

                    // Detect closes — collect first to avoid split borrow.
                    let closed_events: Vec<PortEvent> = st
                        .prev_keys
                        .iter()
                        .filter(|key| !new_keys.contains(*key))
                        .map(|key| {
                            let name = st
                                .listeners
                                .iter()
                                .find(|l| &l.key() == key)
                                .map(|l| l.process_name.clone())
                                .unwrap_or_else(|| "?".to_string());
                            PortEvent {
                                port: key.port,
                                protocol: key.protocol.clone(),
                                process_name: name,
                                kind: EventKind::Closed,
                                ts_ms: now,
                            }
                        })
                        .collect();
                    for ev in closed_events {
                        if st.activity.len() >= ACTIVITY_RING {
                            st.activity.remove(0);
                        }
                        st.activity.push(ev);
                    }

                    st.prev_keys = new_keys;
                    st.listeners = new_listeners;
                    st.last_snap = Some(snap);
                }
            }
        }
    });
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

/// Colour for a port number — well-known ports stand out.
fn port_color(port: u16) -> Color {
    match port {
        22 | 23 => Color::RED,                            // ssh / telnet
        80 | 443 | 8080 | 8443 => Color::GREEN,           // http/https
        5432 | 3306 | 27017 | 6379 | 5672 => Color::CYAN, // db/cache
        _ => Color::WHITE,
    }
}

fn exposure_badge(exposed: bool) -> (&'static str, Color) {
    if exposed {
        ("EXPOSED", Color::YELLOW)
    } else {
        ("local ", Color::DARK_GRAY)
    }
}

// ── portwatch (full) ──────────────────────────────────────────────────────────

fn build_portwatch(w: u16, h: u16) -> Widget {
    tick();

    STORE.with(|s| {
        let Ok(st) = s.try_borrow() else {
            return error_widget("PORTWATCH", "store busy");
        };

        if let Some(ref e) = st.error {
            return error_widget("PORTWATCH", e);
        }
        let Some(ref snap) = st.last_snap else {
            return loading_widget("PORTWATCH", "Scanning ports…");
        };

        let mut lines: Vec<Line> = Vec::new();

        // ── Header ───────────────────────────────────────────────────────────
        let n_listen = st.listeners.len();
        let n_exposed = st.listeners.iter().filter(|l| l.exposed).count();
        lines.push(Line::new(vec![
            ui::span("PORTWATCH", Style::default().bold()),
            ui::span("  ", Style::default()),
            ui::span(
                format!("{n_listen} LISTENING"),
                Style::fg(if n_listen > 0 {
                    Color::WHITE
                } else {
                    Color::DARK_GRAY
                }),
            ),
            ui::span("  ", Style::default()),
            ui::span(
                format!("{n_exposed} EXPOSED"),
                Style::fg(if n_exposed > 0 {
                    Color::YELLOW
                } else {
                    Color::DARK_GRAY
                }),
            ),
            ui::span("  ", Style::default()),
            ui::span(format!("{} ESTABLISHED", snap.established), Style::dim()),
        ]));
        lines.push(sep(w));

        if st.listeners.is_empty() {
            lines.push(Line::text("  no listening ports found", Style::dim()));
        } else {
            // ── Listener table ────────────────────────────────────────────────
            let name_w = (w as usize).saturating_sub(30).clamp(10, 20);
            let mut table = Table::new(&[
                ("PORT", 6, true),
                ("PROTO", 5, false),
                ("PROCESS", name_w, false),
                ("PID", 7, true),
                ("SCOPE", 7, false),
            ]);

            let table_rows = (h as usize).saturating_sub(8).clamp(1, st.listeners.len());
            for l in st.listeners.iter().take(table_rows) {
                let (badge, badge_col) = exposure_badge(l.exposed);
                let port_col = port_color(l.port);
                table.row(vec![
                    (l.port.to_string(), Style::fg(port_col)),
                    (l.protocol.to_uppercase(), Style::dim()),
                    (l.process_name.clone(), Style::fg(Color::WHITE)),
                    (
                        l.pid.map(|p| p.to_string()).unwrap_or_default(),
                        Style::dim(),
                    ),
                    (badge.to_string(), Style::fg(badge_col)),
                ]);
            }
            for ln in table.lines() {
                lines.push(ln);
            }

            if st.listeners.len() > table_rows {
                lines.push(Line::text(
                    format!("  … {} more", st.listeners.len() - table_rows),
                    Style::dim(),
                ));
            }
        }

        // ── Activity feed ─────────────────────────────────────────────────────
        if !st.activity.is_empty() && (h as usize) > lines.len() + 3 {
            lines.push(Line::blank());
            lines.push(Line::text("PORT ACTIVITY", Style::dim()));

            let now = now_ms();
            let rows = (h as usize).saturating_sub(lines.len() + 1).clamp(1, 8);
            // Show newest first.
            for ev in st.activity.iter().rev().take(rows) {
                let (sign, col) = match ev.kind {
                    EventKind::Opened => ("+", Color::GREEN),
                    EventKind::Closed => ("-", Color::RED),
                };
                let name_w = (w as usize).saturating_sub(26).clamp(6, 14);
                lines.push(Line::new(vec![
                    ui::span(sign, Style::fg(col).bold()),
                    ui::span(" ", Style::default()),
                    ui::span(
                        format!("{}/{}", ev.port, ev.protocol),
                        Style::fg(port_color(ev.port)),
                    ),
                    ui::span(
                        format!(
                            "  {:<w$}",
                            ui::truncate(&ev.process_name, name_w),
                            w = name_w
                        ),
                        Style::fg(Color::WHITE),
                    ),
                    ui::span(format!("  {}", ev.age_str(now)), Style::dim()),
                ]));
            }
        } else if st.activity.is_empty() && (h as usize) > lines.len() + 2 {
            lines.push(Line::blank());
            lines.push(Line::text("PORT ACTIVITY", Style::dim()));
            lines.push(Line::text("  no changes detected yet", Style::dim()));
        }

        Widget::paragraph(lines).block(Block::titled(" PORTWATCH ".to_string()))
    })
}

// ── port_listeners (compact) ──────────────────────────────────────────────────

fn build_port_listeners(w: u16, h: u16) -> Widget {
    tick();

    STORE.with(|s| {
        let Ok(st) = s.try_borrow() else {
            return error_widget("PORTWATCH", "store busy");
        };

        if let Some(ref e) = st.error {
            return error_widget("LISTENERS", e);
        }
        if st.last_snap.is_none() {
            return loading_widget("LISTENERS", "scanning…");
        }

        let n = st.listeners.len();
        let n_exposed = st.listeners.iter().filter(|l| l.exposed).count();
        let mut lines = vec![Line::new(vec![
            ui::span("LISTENERS ", Style::default().bold()),
            ui::span(n.to_string(), Style::fg(Color::WHITE)),
            ui::span("  exposed ", Style::dim()),
            ui::span(
                n_exposed.to_string(),
                Style::fg(if n_exposed > 0 {
                    Color::YELLOW
                } else {
                    Color::DARK_GRAY
                }),
            ),
        ])];

        if st.listeners.is_empty() {
            lines.push(Line::text("  none", Style::dim()));
        } else {
            let rows = (h as usize).saturating_sub(1).clamp(1, st.listeners.len());
            let name_w = (w as usize).saturating_sub(18).clamp(6, 16);
            for l in st.listeners.iter().take(rows) {
                let (badge, badge_col) = exposure_badge(l.exposed);
                lines.push(Line::new(vec![
                    ui::span(
                        format!("{:>5}", l.port),
                        Style::fg(port_color(l.port)).bold(),
                    ),
                    ui::span("/tcp  ", Style::dim()),
                    ui::span(
                        format!("{:<w$}", ui::truncate(&l.process_name, name_w), w = name_w),
                        Style::fg(Color::WHITE),
                    ),
                    ui::span(format!("  {badge}"), Style::fg(badge_col)),
                ]));
            }
        }

        Widget::paragraph(lines).block(Block::titled(" LISTENERS ".to_string()))
    })
}

// ── port_activity (feed only) ─────────────────────────────────────────────────

fn build_port_activity(w: u16, h: u16) -> Widget {
    tick();

    STORE.with(|s| {
        let Ok(st) = s.try_borrow() else {
            return error_widget("PORTWATCH", "store busy");
        };

        if let Some(ref e) = st.error {
            return error_widget("PORT ACTIVITY", e);
        }

        let mut lines = vec![Line::new(vec![ui::span(
            "PORT ACTIVITY",
            Style::default().bold(),
        )])];

        if st.activity.is_empty() {
            lines.push(Line::text("  no changes detected yet", Style::dim()));
            lines.push(Line::text(
                "  (watching for open/close events)",
                Style::dim(),
            ));
        } else {
            let now = now_ms();
            let rows = (h as usize).saturating_sub(1).clamp(1, st.activity.len());
            let name_w = (w as usize).saturating_sub(28).clamp(6, 14);
            for ev in st.activity.iter().rev().take(rows) {
                let (sign, col) = match ev.kind {
                    EventKind::Opened => ("+", Color::GREEN),
                    EventKind::Closed => ("-", Color::RED),
                };
                lines.push(Line::new(vec![
                    ui::span(sign, Style::fg(col).bold()),
                    ui::span(" ", Style::default()),
                    ui::span(
                        format!("{:>5}/{}", ev.port, ev.protocol),
                        Style::fg(port_color(ev.port)),
                    ),
                    ui::span(
                        format!(
                            "  {:<w$}",
                            ui::truncate(&ev.process_name, name_w),
                            w = name_w
                        ),
                        Style::fg(Color::WHITE),
                    ),
                    ui::span(format!("  {}", ev.age_str(now)), Style::dim()),
                ]));
            }
        }

        Widget::paragraph(lines).block(Block::titled(" PORT ACTIVITY ".to_string()))
    })
}

// ── Plugin exports ────────────────────────────────────────────────────────────


#[plugin_fn]
pub fn metadata() -> FnResult<Vec<u8>> {
    Ok(vanta_ext_sdk::ExtensionMetadata::new(
        "portwatch_activity",
        "Portwatch Activity",
        "0.1.0",
        "Micro-extension",
        vanta_ext_sdk::API_VERSION_TELEMETRY,
    )
    .to_json())
}

#[plugin_fn]
pub fn widgets(_: ()) -> FnResult<Vec<u8>> {
    let ids = serde_json::json!(["port_activity"]);
    Ok(serde_json::to_vec(&ids).unwrap_or_default())
}

#[plugin_fn]
pub fn render_widget(id: String) -> FnResult<Vec<u8>> {
    if id != "port_activity" {
        return Ok(vanta_ext_sdk::ui::unavailable("UNKNOWN", "invalid widget").to_json());
    }
    let widget = build_port_activity(80, 24);
    Ok(widget.to_json())
}
