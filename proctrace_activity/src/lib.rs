use extism_pdk::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use vanta_ext_sdk::history::now_ms;
use vanta_ext_sdk::telemetry::{self, ProcessTreeNode};
use vanta_ext_sdk::ui::{Block, Color, Line, Span, Style, Widget};

const REFRESH_MS: u64 = 1000;
const ACTIVITY_RING: usize = 20;

#[derive(Debug, Clone, PartialEq)]
enum EventKind {
    Started,
    Exited,
}

#[derive(Debug, Clone)]
struct ProcEvent {
    pid: u32,
    ppid: u32,
    name: String,
    kind: EventKind,
    ts_ms: u64,
}

#[derive(Debug, Clone)]
struct Node {
    proc: ProcessTreeNode,
    children: Vec<u32>,
}

#[derive(Default)]
struct Store {
    caps_done: bool,
    available: bool,
    last_fetch_ms: u64,

    nodes: HashMap<u32, Node>,
    roots: Vec<u32>,
    prev_pids: HashSet<u32>,
    activity: Vec<ProcEvent>,

    error: Option<String>,
}

thread_local! {
    static STORE: RefCell<Store> = RefCell::new(Store::default());
}

fn format_age(diff_ms: u64) -> String {
    let secs = diff_ms / 1000;
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h", secs / 3600)
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
                    st.available = c.has("process_tree");
                }
            }
        });
    }

    let (due, available) = STORE.with(|s| match s.try_borrow() {
        Ok(st) => (
            now.saturating_sub(st.last_fetch_ms) >= REFRESH_MS,
            st.available,
        ),
        Err(_) => (false, false),
    });

    if !due || !available {
        return;
    }

    let result = telemetry::process_tree();

    STORE.with(|s| {
        if let Ok(mut st) = s.try_borrow_mut() {
            st.last_fetch_ms = now;

            match result {
                Err(e) => st.error = Some(e.to_string()),
                Ok(snap) => {
                    st.error = None;

                    let mut nodes: HashMap<u32, Node> = HashMap::new();
                    let mut current_pids = HashSet::new();

                    for p in snap.processes {
                        current_pids.insert(p.pid);
                        nodes.insert(
                            p.pid,
                            Node {
                                proc: p,
                                children: Vec::new(),
                            },
                        );
                    }

                    // Build tree
                    let mut roots = Vec::new();
                    let mut children_map: HashMap<u32, Vec<u32>> = HashMap::new();

                    for p in nodes.values() {
                        let ppid = p.proc.ppid;
                        let pid = p.proc.pid;
                        if ppid == 0 || !nodes.contains_key(&ppid) {
                            roots.push(pid);
                        } else {
                            children_map.entry(ppid).or_default().push(pid);
                        }
                    }

                    for (ppid, children) in children_map {
                        if let Some(node) = nodes.get_mut(&ppid) {
                            node.children = children;
                            node.children.sort_unstable(); // Deterministic ordering
                        }
                    }

                    roots.sort_unstable(); // Deterministic ordering

                    // Detect Starts
                    for &pid in &current_pids {
                        if !st.prev_pids.contains(&pid) {
                            let proc = &nodes[&pid].proc;
                            let ev = ProcEvent {
                                pid,
                                ppid: proc.ppid,
                                name: proc.name.clone(),
                                kind: EventKind::Started,
                                ts_ms: now,
                            };
                            if st.activity.len() >= ACTIVITY_RING {
                                st.activity.remove(0);
                            }
                            st.activity.push(ev);
                        }
                    }

                    // Detect Exits
                    let mut closed_events = Vec::new();
                    let prev = st.prev_pids.clone();
                    for pid in prev {
                        if !current_pids.contains(&pid) {
                            // Find name from previous state if possible
                            let name = st
                                .nodes
                                .get(&pid)
                                .map(|n| n.proc.name.clone())
                                .unwrap_or_else(|| "?".to_string());
                            let ppid = st.nodes.get(&pid).map(|n| n.proc.ppid).unwrap_or(0);
                            closed_events.push(ProcEvent {
                                pid,
                                ppid,
                                name,
                                kind: EventKind::Exited,
                                ts_ms: now,
                            });
                        }
                    }

                    for ev in closed_events {
                        if st.activity.len() >= ACTIVITY_RING {
                            st.activity.remove(0);
                        }
                        st.activity.push(ev);
                    }

                    st.prev_pids = current_pids;
                    st.nodes = nodes;
                    st.roots = roots;
                }
            }
        }
    });
}

fn error_widget(title: &str, msg: &str) -> Widget {
    Widget::paragraph(vec![
        Line::new(vec![Span {
            content: format!("{title} — error"),
            style: Some(Style::fg(Color::RED).bold()),
        }]),
        Line::new(vec![Span {
            content: msg.to_string(),
            style: Some(Style::dim()),
        }]),
    ])
    .block(Block::titled(format!(" {title} ")))
}

fn render_node(
    pid: u32,
    nodes: &HashMap<u32, Node>,
    lines: &mut Vec<Line>,
    prefix: String,
    is_last: bool,
    height: usize,
    _max_width: usize,
) {
    if lines.len() >= height {
        return;
    }

    let Some(node) = nodes.get(&pid) else { return };

    let branch = if prefix.is_empty() {
        ""
    } else if is_last {
        "└─ "
    } else {
        "├─ "
    };

    let state_col = if node.proc.state == "R" {
        Color::GREEN
    } else if node.proc.state == "Z" {
        Color::RED
    } else {
        Color::DARK_GRAY
    };

    let name = format!(
        "{:<14}",
        node.proc.name.chars().take(13).collect::<String>()
    );
    let pid_str = format!("{:>6}", pid);
    let state_str = format!("{:<2}", node.proc.state.chars().take(2).collect::<String>());
    let threads_str = format!("{:>3}", node.proc.threads);
    let uid_str = format!("{:>4}", node.proc.uid);

    lines.push(Line::new(vec![
        Span {
            content: format!("{prefix}{branch}"),
            style: Some(Style::dim()),
        },
        Span {
            content: name,
            style: Some(Style::fg(Color::WHITE).bold()),
        },
        Span {
            content: " ".into(),
            style: None,
        },
        Span {
            content: pid_str,
            style: Some(Style::fg(Color::CYAN)),
        },
        Span {
            content: " ".into(),
            style: None,
        },
        Span {
            content: state_str,
            style: Some(Style::fg(state_col)),
        },
        Span {
            content: " ".into(),
            style: None,
        },
        Span {
            content: format!("{}t", threads_str),
            style: Some(Style::dim()),
        },
        Span {
            content: " ".into(),
            style: None,
        },
        Span {
            content: format!("u{}", uid_str),
            style: Some(Style::dim()),
        },
    ]));

    let new_prefix = if prefix.is_empty() {
        "".to_string()
    } else if is_last {
        format!("{prefix}   ")
    } else {
        format!("{prefix}│  ")
    };

    for (i, &child_pid) in node.children.iter().enumerate() {
        if lines.len() >= height {
            break;
        }
        let child_is_last = i == node.children.len() - 1;
        render_node(
            child_pid,
            nodes,
            lines,
            new_prefix.clone(),
            child_is_last,
            height,
            _max_width,
        );
    }
}

fn build_proctrace(width: u16, height: u16) -> Widget {
    tick();

    let Ok(st) = STORE.try_with(|s| {
        s.try_borrow().map(|g| {
            (
                g.error.clone(),
                g.available,
                g.nodes.clone(),
                g.roots.clone(),
            )
        })
    }) else {
        return error_widget("PROCTRACE", "store busy");
    };

    let Ok((err, avail, nodes, roots)) = st else {
        return error_widget("PROCTRACE", "store busy");
    };

    if !avail {
        return error_widget("PROCTRACE", "process_tree unavailable");
    }
    if let Some(e) = err {
        return error_widget("PROCTRACE", &e);
    }
    if nodes.is_empty() {
        return error_widget("PROCTRACE", "Loading...");
    }

    let mut lines = Vec::new();
    lines.push(Line::new(vec![Span {
        content: "TREE           NAME            PID ST THR   UID".into(),
        style: Some(Style::dim()),
    }]));
    lines.push(Line::new(vec![Span {
        content: "─".repeat(width as usize),
        style: Some(Style::dim()),
    }]));

    let max_lines = height.saturating_sub(2) as usize;

    for (i, &root_pid) in roots.iter().enumerate() {
        if lines.len() >= max_lines + 2 {
            break;
        }
        render_node(
            root_pid,
            &nodes,
            &mut lines,
            "".to_string(),
            i == roots.len() - 1,
            max_lines + 2,
            width as usize,
        );
    }

    Widget::paragraph(lines).block(Block::titled(" PROCTRACE ".to_string()))
}

fn build_ancestry(pid: u32, width: u16, height: u16) -> Widget {
    tick();

    let Ok(st) = STORE.try_with(|s| {
        s.try_borrow()
            .map(|g| (g.error.clone(), g.available, g.nodes.clone()))
    }) else {
        return error_widget("ANCESTRY", "store busy");
    };

    let Ok((err, avail, nodes)) = st else {
        return error_widget("ANCESTRY", "store busy");
    };

    if !avail {
        return error_widget("ANCESTRY", "unavailable");
    }
    if let Some(e) = err {
        return error_widget("ANCESTRY", &e);
    }
    if nodes.is_empty() {
        return error_widget("ANCESTRY", "Loading...");
    }

    let mut lines = Vec::new();
    lines.push(Line::new(vec![Span {
        content: "ANCESTRY       NAME            PID ST THR   UID".into(),
        style: Some(Style::dim()),
    }]));
    lines.push(Line::new(vec![Span {
        content: "─".repeat(width as usize),
        style: Some(Style::dim()),
    }]));

    if !nodes.contains_key(&pid) {
        lines.push(Line::new(vec![Span {
            content: format!("Process {} not found", pid),
            style: Some(Style::fg(Color::RED)),
        }]));
        return Widget::paragraph(lines).block(Block::titled(" PROCTRACE ANCESTRY ".to_string()));
    }

    let mut path = Vec::new();
    let mut curr = pid;
    while let Some(node) = nodes.get(&curr) {
        path.push(curr);
        if node.proc.ppid == 0 || node.proc.ppid == curr || !nodes.contains_key(&node.proc.ppid) {
            break;
        }
        curr = node.proc.ppid;
    }
    path.reverse();

    let max_lines = height.saturating_sub(2) as usize;
    let skip = path.len().saturating_sub(max_lines);

    for (i, &p) in path.iter().skip(skip).enumerate() {
        let node = &nodes[&p];
        let prefix = "  ".repeat(i);
        let branch = if i == 0 { "" } else { "└─ " };

        let state_col = if node.proc.state == "R" {
            Color::GREEN
        } else {
            Color::DARK_GRAY
        };
        let name = format!(
            "{:<14}",
            node.proc.name.chars().take(13).collect::<String>()
        );
        let pid_str = format!("{:>6}", p);
        let state_str = format!("{:<2}", node.proc.state.chars().take(2).collect::<String>());

        lines.push(Line::new(vec![
            Span {
                content: format!("{prefix}{branch}"),
                style: Some(Style::dim()),
            },
            Span {
                content: name,
                style: Some(if p == pid {
                    Style::fg(Color::CYAN).bold()
                } else {
                    Style::fg(Color::WHITE)
                }),
            },
            Span {
                content: " ".into(),
                style: None,
            },
            Span {
                content: pid_str,
                style: Some(Style::fg(Color::CYAN)),
            },
            Span {
                content: " ".into(),
                style: None,
            },
            Span {
                content: state_str,
                style: Some(Style::fg(state_col)),
            },
        ]));
    }

    Widget::paragraph(lines).block(Block::titled(format!(" ANCESTRY {} ", pid)))
}

fn build_descendants(pid: u32, width: u16, height: u16) -> Widget {
    tick();

    let Ok(st) = STORE.try_with(|s| {
        s.try_borrow()
            .map(|g| (g.error.clone(), g.available, g.nodes.clone()))
    }) else {
        return error_widget("DESCENDANTS", "store busy");
    };

    let Ok((err, avail, nodes)) = st else {
        return error_widget("DESCENDANTS", "store busy");
    };

    if !avail {
        return error_widget("DESCENDANTS", "unavailable");
    }
    if let Some(e) = err {
        return error_widget("DESCENDANTS", &e);
    }
    if nodes.is_empty() {
        return error_widget("DESCENDANTS", "Loading...");
    }

    let mut lines = Vec::new();
    lines.push(Line::new(vec![Span {
        content: "SUBTREE        NAME            PID ST THR   UID".into(),
        style: Some(Style::dim()),
    }]));
    lines.push(Line::new(vec![Span {
        content: "─".repeat(width as usize),
        style: Some(Style::dim()),
    }]));

    if !nodes.contains_key(&pid) {
        lines.push(Line::new(vec![Span {
            content: format!("Process {} not found", pid),
            style: Some(Style::fg(Color::RED)),
        }]));
        return Widget::paragraph(lines).block(Block::titled(" PROCTRACE SUBTREE ".to_string()));
    }

    let max_lines = height.saturating_sub(2) as usize;
    render_node(
        pid,
        &nodes,
        &mut lines,
        "".to_string(),
        true,
        max_lines + 2,
        width as usize,
    );

    Widget::paragraph(lines).block(Block::titled(format!(" SUBTREE {} ", pid)))
}

fn build_activity(_width: u16, height: u16) -> Widget {
    tick();

    let Ok(st) = STORE.try_with(|s| {
        s.try_borrow()
            .map(|g| (g.error.clone(), g.available, g.activity.clone()))
    }) else {
        return error_widget("ACTIVITY", "store busy");
    };

    let Ok((err, avail, activity)) = st else {
        return error_widget("ACTIVITY", "store busy");
    };

    if !avail {
        return error_widget("ACTIVITY", "unavailable");
    }
    if let Some(e) = err {
        return error_widget("ACTIVITY", &e);
    }

    let mut lines = Vec::new();
    lines.push(Line::new(vec![Span {
        content: "ACTIVITY".into(),
        style: Some(Style::dim()),
    }]));

    let now = now_ms();
    let max_act = height.saturating_sub(2) as usize;

    for ev in activity.iter().rev().take(max_act.max(1)) {
        let sign = if ev.kind == EventKind::Started {
            "+"
        } else {
            "-"
        };
        let sign_color = if ev.kind == EventKind::Started {
            Color::GREEN
        } else {
            Color::RED
        };
        let name = format!("{:<15}", ev.name.chars().take(14).collect::<String>());
        let age_str = format_age(now.saturating_sub(ev.ts_ms));

        lines.push(Line::new(vec![
            Span {
                content: format!("{} ", sign),
                style: Some(Style::fg(sign_color).bold()),
            },
            Span {
                content: name,
                style: Some(Style::fg(Color::WHITE).bold()),
            },
            Span {
                content: format!("{:>6} ", ev.pid),
                style: Some(Style::fg(Color::CYAN)),
            },
            Span {
                content: format!("(ppid {:>5}) ", ev.ppid),
                style: Some(Style::dim()),
            },
            Span {
                content: format!("{:>4}", age_str),
                style: Some(Style::dim()),
            },
        ]));
    }

    Widget::paragraph(lines).block(Block::titled(" PROCTRACE ACTIVITY ".to_string()))
}


#[plugin_fn]
pub fn metadata() -> FnResult<Vec<u8>> {
    Ok(vanta_ext_sdk::ExtensionMetadata::new(
        "proctrace_activity",
        "Proctrace Activity",
        "0.1.0",
        "Micro-extension",
        vanta_ext_sdk::API_VERSION_TELEMETRY,
    )
    .to_json())
}

#[plugin_fn]
pub fn widgets(_: ()) -> FnResult<Vec<u8>> {
    let ids = serde_json::json!(["proctrace_activity"]);
    Ok(serde_json::to_vec(&ids).unwrap_or_default())
}

#[plugin_fn]
pub fn render_widget(id: String) -> FnResult<Vec<u8>> {
    if id != "proctrace_activity" {
        return Ok(vanta_ext_sdk::ui::unavailable("UNKNOWN", "invalid widget").to_json());
    }
    let widget = build_activity(80, 24);
    Ok(widget.to_json())
}
