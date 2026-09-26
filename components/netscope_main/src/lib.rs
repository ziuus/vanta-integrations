use extism_pdk::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use vanta_ext_sdk::history::now_ms;
use vanta_ext_sdk::telemetry::{self, ConnectionEntry};
use vanta_ext_sdk::ui::{Block, Color, Line, Span, Style, Widget};

const REFRESH_MS: u64 = 800;
const ACTIVITY_RING: usize = 20;

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
struct ConnKey {
    protocol: String,
    local_addr: String,
    remote_addr: String,
}

#[derive(Debug, Clone, PartialEq)]
enum EventKind {
    Opened,
    Closed,
}

#[derive(Debug, Clone)]
struct NetEvent {
    process_name: String,
    remote_addr: String,
    kind: EventKind,
    ts_ms: u64,
}

#[derive(Default)]
struct Store {
    connections_available: bool,
    caps_done: bool,
    last_fetch_ms: u64,

    connections: Vec<ConnectionEntry>,
    counts: HashMap<String, usize>,

    prev_keys: HashSet<ConnKey>,
    activity: Vec<NetEvent>,

    error: Option<String>,
}

thread_local! {
    static STORE: RefCell<Store> = RefCell::new(Store::default());
}

fn is_listen(addr: &str) -> bool {
    addr.starts_with("0.0.0.0:") || addr.starts_with("[::]:")
}

fn format_state(s: &str) -> &str {
    match s {
        "ESTABLISHED" => "ESTAB",
        "TIME_WAIT" => "TIME_W",
        "CLOSE_WAIT" => "CLO_W",
        "FIN_WAIT1" => "FIN_W1",
        "FIN_WAIT2" => "FIN_W2",
        "LISTEN" => "LISTEN",
        _ => s,
    }
}

fn format_age(diff_ms: u64) -> String {
    let secs = diff_ms / 1000;
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m", secs / 60)
    }
}

fn tick() {
    let now = now_ms();

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

    let (due, available) = STORE.with(|s| match s.try_borrow() {
        Ok(st) => (
            now.saturating_sub(st.last_fetch_ms) >= REFRESH_MS,
            st.connections_available,
        ),
        Err(_) => (false, false),
    });

    if !due || !available {
        return;
    }

    let result = telemetry::connections();

    STORE.with(|s| {
        if let Ok(mut st) = s.try_borrow_mut() {
            st.last_fetch_ms = now;

            match result {
                Err(e) => st.error = Some(e.to_string()),
                Ok(snap) => {
                    st.error = None;

                    let mut counts = HashMap::new();
                    let mut new_keys = HashSet::new();
                    let mut valid_conns = Vec::new();

                    for c in snap.connections {
                        let key = ConnKey {
                            protocol: c.protocol.clone(),
                            local_addr: c.local_addr.clone(),
                            remote_addr: c.remote_addr.clone(),
                        };
                        new_keys.insert(key);
                        *counts.entry(c.state.clone()).or_insert(0) += 1;
                        valid_conns.push(c);
                    }

                    valid_conns.sort_by(|a, b| {
                        let rank = |s: &str| match s {
                            "ESTABLISHED" => 0,
                            "LISTEN" => 1,
                            _ => 2,
                        };
                        rank(&a.state)
                            .cmp(&rank(&b.state))
                            .then(a.process_name.cmp(&b.process_name))
                    });

                    // Detect Opens
                    for c in &valid_conns {
                        let key = ConnKey {
                            protocol: c.protocol.clone(),
                            local_addr: c.local_addr.clone(),
                            remote_addr: c.remote_addr.clone(),
                        };
                        if !st.prev_keys.contains(&key) && c.state != "LISTEN" {
                            let ev = NetEvent {
                                process_name: c
                                    .process_name
                                    .clone()
                                    .unwrap_or_else(|| "?".to_string()),
                                remote_addr: c.remote_addr.clone(),
                                kind: EventKind::Opened,
                                ts_ms: now,
                            };
                            if st.activity.len() >= ACTIVITY_RING {
                                st.activity.remove(0);
                            }
                            st.activity.push(ev);
                        }
                    }

                    // Detect Closes
                    let closed_events: Vec<NetEvent> = st
                        .prev_keys
                        .iter()
                        .filter(|k| !new_keys.contains(*k))
                        .filter(|k| !is_listen(&k.remote_addr))
                        .map(|k| {
                            let name = st
                                .connections
                                .iter()
                                .find(|c| {
                                    c.protocol == k.protocol
                                        && c.local_addr == k.local_addr
                                        && c.remote_addr == k.remote_addr
                                })
                                .and_then(|c| c.process_name.clone())
                                .unwrap_or_else(|| "?".to_string());
                            NetEvent {
                                process_name: name,
                                remote_addr: k.remote_addr.clone(),
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
                    st.connections = valid_conns;
                    st.counts = counts;
                }
            }
        }
    });
}

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

fn build_netscope(
    width: u16,
    height: u16,
    show_table: bool,
    show_summary: bool,
    show_activity: bool,
) -> Widget {
    tick();

    let Ok(st) = STORE.try_with(|s| {
        s.try_borrow().map(|g| {
            (
                g.error.clone(),
                g.connections_available,
                g.connections.clone(),
                g.counts.clone(),
                g.activity.clone(),
            )
        })
    }) else {
        return error_widget("NETSCOPE", "store busy");
    };

    let Ok((err, avail, conns, counts, activity)) = st else {
        return error_widget("NETSCOPE", "store busy");
    };

    if !avail {
        return error_widget("NETSCOPE", "host API 'connections' unavailable");
    }
    if let Some(e) = err {
        return error_widget("NETSCOPE", &e);
    }
    if conns.is_empty() && activity.is_empty() {
        return loading_widget("NETSCOPE", "Awaiting connections...");
    }

    let mut lines = Vec::new();

    if show_table {
        lines.push(Line::text(
            "PROCESS        LOCAL              REMOTE                 STATE",
            Style::dim(),
        ));
        lines.push(sep(width));

        let max_rows = if show_activity || show_summary {
            10
        } else {
            height.saturating_sub(4) as usize
        };

        for c in conns.iter().take(max_rows.max(1)) {
            let proc_str = c.process_name.as_deref().unwrap_or("?");

            let name_col = format!("{:<14}", proc_str.chars().take(13).collect::<String>());
            let local_col = format!("{:<18}", c.local_addr.chars().take(17).collect::<String>());
            let remote_col = format!("{:<22}", c.remote_addr.chars().take(21).collect::<String>());
            let state_str = format_state(&c.state);
            let state_col = format!("{:<7}", state_str.chars().take(6).collect::<String>());

            let color = match state_str {
                "ESTAB" => Color::GREEN,
                "LISTEN" => Color::CYAN,
                "TIME_W" => Color::DARK_GRAY,
                "CLO_W" => Color::YELLOW,
                _ => Color::WHITE,
            };

            lines.push(Line::new(vec![
                Span {
                    content: name_col,
                    style: Some(Style::fg(Color::WHITE).bold()),
                },
                Span {
                    content: local_col,
                    style: Some(Style::dim()),
                },
                Span {
                    content: remote_col,
                    style: Some(Style::fg(Color::WHITE)),
                },
                Span {
                    content: state_col,
                    style: Some(Style::fg(color)),
                },
            ]));
        }
    }

    if show_summary {
        if show_table {
            lines.push(Line::blank());
        }
        lines.push(Line::text("CONNECTIONS", Style::dim()));
        let mut sorted_counts: Vec<_> = counts.into_iter().collect();
        sorted_counts.sort_by_key(|a| std::cmp::Reverse(a.1));

        for (state, count) in sorted_counts {
            let state_str = format_state(&state);
            let color = match state_str {
                "ESTAB" => Color::GREEN,
                "LISTEN" => Color::CYAN,
                _ => Color::DARK_GRAY,
            };
            lines.push(Line::new(vec![
                Span {
                    content: format!("{:<13}", state_str),
                    style: Some(Style::fg(color)),
                },
                Span {
                    content: format!("{}", count),
                    style: Some(Style::fg(Color::WHITE).bold()),
                },
            ]));
        }
    }

    if show_activity {
        if show_table || show_summary {
            lines.push(Line::blank());
        }
        lines.push(Line::text("ACTIVITY", Style::dim()));

        let now = now_ms();
        let max_act = if show_table || show_summary {
            6
        } else {
            height.saturating_sub(4) as usize
        };

        for ev in activity.iter().rev().take(max_act.max(1)) {
            let sign = if ev.kind == EventKind::Opened {
                "+"
            } else {
                "-"
            };
            let sign_color = if ev.kind == EventKind::Opened {
                Color::GREEN
            } else {
                Color::RED
            };
            let proc_str = format!("{:<6}", ev.process_name.chars().take(5).collect::<String>());
            let remote_str = format!(
                "{:<21}",
                ev.remote_addr.chars().take(20).collect::<String>()
            );
            let age_str = format_age(now.saturating_sub(ev.ts_ms));

            lines.push(Line::new(vec![
                Span {
                    content: format!("{} ", sign),
                    style: Some(Style::fg(sign_color).bold()),
                },
                Span {
                    content: proc_str,
                    style: Some(Style::fg(Color::WHITE).bold()),
                },
                Span {
                    content: " → ".into(),
                    style: Some(Style::dim()),
                },
                Span {
                    content: remote_str,
                    style: Some(Style::fg(Color::WHITE)),
                },
                Span {
                    content: format!("{:>4}", age_str),
                    style: Some(Style::dim()),
                },
            ]));
        }
    }

    let title = if show_table && show_summary && show_activity {
        " NETSCOPE "
    } else if show_table {
        " NETSCOPE TABLE "
    } else if show_summary {
        " NETSCOPE SUMMARY "
    } else {
        " NETSCOPE ACTIVITY "
    };

    Widget::paragraph(lines).block(Block::titled(title.to_string()))
}


#[plugin_fn]
pub fn metadata() -> FnResult<Vec<u8>> {
    Ok(vanta_ext_sdk::ExtensionMetadata::new(
        "netscope_main",
        "Netscope Main",
        "0.1.0",
        "Micro-extension",
        vanta_ext_sdk::API_VERSION_TELEMETRY,
    )
    .to_json())
}

#[plugin_fn]
pub fn widgets(_: ()) -> FnResult<Vec<u8>> {
    let ids = serde_json::json!(["netscope", "netscope_table", "netscope_summary", "netscope_activity"]);
    Ok(serde_json::to_vec(&ids).unwrap_or_default())
}

#[plugin_fn]
pub fn render_widget(id: String) -> FnResult<Vec<u8>> {
    let widget = match id.as_str() {
        "netscope" => build_netscope(80, 24, true, true, true),
        "netscope_table" => build_netscope(80, 24, true, false, false),
        "netscope_summary" => build_netscope(80, 24, false, true, false),
        "netscope_activity" => build_netscope(80, 24, false, false, true),
        _ => return Ok(vanta_ext_sdk::ui::unavailable("UNKNOWN", "invalid widget").to_json()),
    };
    Ok(widget.to_json())
}
