use extism_pdk::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use vanta_ext_sdk::history::now_ms;
use vanta_ext_sdk::telemetry::{self, ServiceNode};
use vanta_ext_sdk::ui::{Block, Color, Line, Span, Style, Widget};

const REFRESH_MS: u64 = 2000;
const ACTIVITY_RING: usize = 20;

#[derive(Debug, Clone, PartialEq)]
enum EventKind { Started, Stopped, Failed }

#[derive(Debug, Clone)]
struct SrvEvent {
    name: String,
    kind: EventKind,
    ts_ms: u64,
}

#[derive(Default)]
struct Store {
    caps_done: bool,
    available: bool,
    last_fetch_ms: u64,

    services: HashMap<String, ServiceNode>,
    prev_states: HashMap<String, String>,
    activity: Vec<SrvEvent>,
    
    error: Option<String>,
}

thread_local! {
    static STORE: RefCell<Store> = RefCell::new(Store::default());
}

fn format_age(diff_ms: u64) -> String {
    let secs = diff_ms / 1000;
    if secs < 60 { format!("{secs}s") }
    else if secs < 3600 { format!("{}m", secs / 60) }
    else { format!("{}h", secs / 3600) }
}

fn strip_suffix(name: &str) -> String {
    name.strip_suffix(".service").unwrap_or(name).to_string()
}

fn tick() {
    let now = now_ms();

    let need_caps = STORE.with(|s| s.try_borrow().map(|st| !st.caps_done).unwrap_or(false));
    if need_caps {
        let caps = telemetry::capabilities();
        STORE.with(|s| {
            if let Ok(mut st) = s.try_borrow_mut() {
                st.caps_done = true;
                if let Ok(c) = caps { st.available = c.has("services"); }
            }
        });
    }

    let (due, available) = STORE.with(|s| match s.try_borrow() {
        Ok(st) => (now.saturating_sub(st.last_fetch_ms) >= REFRESH_MS, st.available),
        Err(_) => (false, false),
    });

    if !due || !available { return; }

    let result = telemetry::services();

    STORE.with(|s| {
        if let Ok(mut st) = s.try_borrow_mut() {
            st.last_fetch_ms = now;

            match result {
                Err(e) => st.error = Some(e.to_string()),
                Ok(snap) => {
                    st.error = None;
                    
                    let mut current = HashMap::new();
                    let mut new_states = HashMap::new();
                    let mut current_names = HashSet::new();

                    for s in snap.services {
                        current_names.insert(s.name.clone());
                        new_states.insert(s.name.clone(), s.active_state.clone());
                        current.insert(s.name.clone(), s);
                    }

                    // Detect Transitions
                    let mut events = Vec::new();
                    
                    for (name, srv) in &current {
                        let prev = st.prev_states.get(name).map(|s| s.as_str()).unwrap_or("inactive");
                        let curr = srv.active_state.as_str();
                        
                        if prev != curr {
                            if curr == "active" {
                                events.push(SrvEvent { name: name.clone(), kind: EventKind::Started, ts_ms: now });
                            } else if curr == "failed" {
                                events.push(SrvEvent { name: name.clone(), kind: EventKind::Failed, ts_ms: now });
                            } else if (curr == "inactive" || curr == "dead") && prev == "active" {
                                events.push(SrvEvent { name: name.clone(), kind: EventKind::Stopped, ts_ms: now });
                            }
                        }
                    }

                    // Missing services that disappeared entirely
                    for (name, prev) in &st.prev_states {
                        if !current_names.contains(name) && prev == "active" {
                            events.push(SrvEvent { name: name.clone(), kind: EventKind::Stopped, ts_ms: now });
                        }
                    }
                    
                    // Sort events to be deterministic, though they are all `now`.
                    events.sort_unstable_by(|a, b| a.name.cmp(&b.name));

                    for ev in events {
                        if st.activity.len() >= ACTIVITY_RING { st.activity.remove(0); }
                        st.activity.push(ev);
                    }

                    st.prev_states = new_states;
                    st.services = current;
                }
            }
        }
    });
}

fn error_widget(title: &str, msg: &str) -> Widget {
    Widget::paragraph(vec![
        Line::new(vec![Span { content: format!("{title} — error"), style: Some(Style::fg(Color::RED).bold()) }]),
        Line::new(vec![Span { content: msg.to_string(), style: Some(Style::dim()) }]),
    ]).block(Block::titled(format!(" {title} ")))
}

fn build_servicewatch(width: u16, height: u16) -> Widget {
    tick();
    
    let Ok(st) = STORE.try_with(|s| s.try_borrow().map(|g| (
        g.error.clone(), g.available, g.services.clone()
    ))) else {
        return error_widget("SERVICEWATCH", "store busy");
    };
    
    let Ok((err, avail, services)) = st else { return error_widget("SERVICEWATCH", "store busy"); };

    if !avail { return error_widget("SERVICEWATCH", "services unavailable"); }
    if let Some(e) = err { return error_widget("SERVICEWATCH", &e); }
    if services.is_empty() { return error_widget("SERVICEWATCH", "Loading..."); }

    let mut lines = Vec::new();
    lines.push(Line::new(vec![
        Span { content: "SERVICE                        STATE    SUBSTATE      PID".into(), style: Some(Style::dim()) }
    ]));
    lines.push(Line::new(vec![Span { content: "─".repeat(width as usize), style: Some(Style::dim()) }]));

    let mut srvs: Vec<_> = services.values().collect();
    // Sort failed first, then active, then alphabetical
    srvs.sort_unstable_by(|a, b| {
        let a_score = match a.active_state.as_str() { "failed" => 0, "active" => 1, _ => 2 };
        let b_score = match b.active_state.as_str() { "failed" => 0, "active" => 1, _ => 2 };
        a_score.cmp(&b_score).then_with(|| a.name.cmp(&b.name))
    });

    let max_lines = height.saturating_sub(2) as usize;
    for srv in srvs.iter().take(max_lines) {
        let name = strip_suffix(&srv.name);
        let name_str = format!("{:<28}", name.chars().take(27).collect::<String>());
        
        let state_col = match srv.active_state.as_str() {
            "active" => Color::GREEN,
            "failed" => Color::RED,
            _ => Color::DARK_GRAY,
        };
        let state_str = format!("{:<8}", srv.active_state.chars().take(8).collect::<String>());
        let sub_str = format!("{:<12}", srv.sub_state.chars().take(12).collect::<String>());
        let pid_str = if srv.pid > 0 { format!("{:>6}", srv.pid) } else { "      ".into() };
        
        lines.push(Line::new(vec![
            Span { content: name_str, style: Some(Style::fg(Color::WHITE).bold()) },
            Span { content: " ".into(), style: None },
            Span { content: state_str, style: Some(Style::fg(state_col)) },
            Span { content: " ".into(), style: None },
            Span { content: sub_str, style: Some(Style::dim()) },
            Span { content: " ".into(), style: None },
            Span { content: pid_str, style: Some(Style::fg(Color::CYAN)) },
        ]));
    }
    
    Widget::paragraph(lines).block(Block::titled(" SERVICEWATCH ".to_string()))
}

fn build_activity(_width: u16, height: u16) -> Widget {
    tick();
    
    let Ok(st) = STORE.try_with(|s| s.try_borrow().map(|g| (
        g.error.clone(), g.available, g.activity.clone()
    ))) else {
        return error_widget("SERVICE ACTIVITY", "store busy");
    };
    
    let Ok((err, avail, activity)) = st else { return error_widget("SERVICE ACTIVITY", "store busy"); };

    if !avail { return error_widget("SERVICE ACTIVITY", "unavailable"); }
    if let Some(e) = err { return error_widget("SERVICE ACTIVITY", &e); }
    
    let mut lines = Vec::new();
    lines.push(Line::new(vec![Span { content: "SERVICE TRANSITIONS".into(), style: Some(Style::dim()) }]));
    
    let now = now_ms();
    let max_act = height.saturating_sub(2) as usize;
    
    for ev in activity.iter().rev().take(max_act.max(1)) {
        let (sign, color) = match ev.kind {
            EventKind::Started => ("+", Color::GREEN),
            EventKind::Stopped => ("-", Color::DARK_GRAY),
            EventKind::Failed => ("!", Color::RED),
        };
        let name = format!("{:<25}", strip_suffix(&ev.name).chars().take(24).collect::<String>());
        let age_str = format_age(now.saturating_sub(ev.ts_ms));
        let kind_str = match ev.kind {
            EventKind::Started => "started",
            EventKind::Stopped => "stopped",
            EventKind::Failed  => "failed ",
        };
        
        lines.push(Line::new(vec![
            Span { content: format!("{} ", sign), style: Some(Style::fg(color.clone()).bold()) },
            Span { content: name, style: Some(Style::fg(Color::WHITE).bold()) },
            Span { content: format!("{} ", kind_str), style: Some(Style::fg(color)) },
            Span { content: format!("{:>4}", age_str), style: Some(Style::dim()) },
        ]));
    }
    
    Widget::paragraph(lines).block(Block::titled(" SERVICE ACTIVITY ".to_string()))
}

#[plugin_fn]
pub fn widgets(_: ()) -> FnResult<Vec<u8>> {
    let ids = serde_json::json!(["servicewatch", "servicewatch_activity"]);
    Ok(serde_json::to_vec(&ids).unwrap_or_default())
}

#[plugin_fn]
pub fn render_widget(widget_id: String) -> FnResult<Vec<u8>> {
    let widget = if widget_id == "servicewatch" {
        build_servicewatch(80, 24)
    } else if widget_id == "servicewatch_activity" {
        build_activity(50, 10)
    } else {
        error_widget("SERVICEWATCH", &format!("unknown widget: {widget_id}"))
    };
    Ok(widget.to_json())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use vanta_ext_sdk::telemetry::ServicesSnapshot;

    fn make_srv(name: &str, active: &str) -> ServiceNode {
        ServiceNode {
            name: name.to_string(),
            load_state: "loaded".into(),
            active_state: active.into(),
            sub_state: "running".into(),
            pid: 100,
            start_ts: 1000,
        }
    }

    fn inject(snap: ServicesSnapshot) {
        STORE.with(|s| {
            let mut st = s.borrow_mut();
            
            let mut current = HashMap::new();
            let mut new_states = HashMap::new();
            let mut current_names = HashSet::new();

            for srv in snap.services {
                current_names.insert(srv.name.clone());
                new_states.insert(srv.name.clone(), srv.active_state.clone());
                current.insert(srv.name.clone(), srv);
            }

            let mut events = Vec::new();
            for (name, srv) in &current {
                let prev = st.prev_states.get(name).map(|s| s.as_str()).unwrap_or("inactive");
                let curr = srv.active_state.as_str();
                if prev != curr {
                    if curr == "active" {
                        events.push(SrvEvent { name: name.clone(), kind: EventKind::Started, ts_ms: 1000 });
                    } else if curr == "failed" {
                        events.push(SrvEvent { name: name.clone(), kind: EventKind::Failed, ts_ms: 1000 });
                    } else if (curr == "inactive" || curr == "dead") && prev == "active" {
                        events.push(SrvEvent { name: name.clone(), kind: EventKind::Stopped, ts_ms: 1000 });
                    }
                }
            }

            for (name, prev) in &st.prev_states {
                if !current_names.contains(name) && prev == "active" {
                    events.push(SrvEvent { name: name.clone(), kind: EventKind::Stopped, ts_ms: 1000 });
                }
            }
            events.sort_unstable_by(|a, b| a.name.cmp(&b.name));
            for ev in events { st.activity.push(ev); }

            st.prev_states = new_states;
            st.services = current;
            st.available = true;
            st.caps_done = true;
            st.last_fetch_ms = now_ms();
            st.error = None;
        });
    }

    #[test]
    fn test_parsing_normalization() {
        assert_eq!(strip_suffix("docker.service"), "docker");
        assert_eq!(strip_suffix("sshd"), "sshd");
    }

    #[test]
    fn test_transitions() {
        STORE.with(|s| { s.borrow_mut().activity.clear(); s.borrow_mut().prev_states.clear(); });
        inject(ServicesSnapshot { total: 1, services: vec![make_srv("a.service", "inactive")] });
        inject(ServicesSnapshot { total: 1, services: vec![make_srv("a.service", "active")] });
        STORE.with(|s| {
            let st = s.borrow();
            assert_eq!(st.activity.len(), 1);
            assert_eq!(st.activity[0].kind, EventKind::Started);
        });

        inject(ServicesSnapshot { total: 1, services: vec![make_srv("a.service", "failed")] });
        STORE.with(|s| {
            let st = s.borrow();
            assert_eq!(st.activity.len(), 2);
            assert_eq!(st.activity[1].kind, EventKind::Failed);
        });
    }

    #[test]
    fn test_disappearing_services() {
        STORE.with(|s| { s.borrow_mut().activity.clear(); s.borrow_mut().prev_states.clear(); });
        inject(ServicesSnapshot { total: 1, services: vec![make_srv("b.service", "active")] });
        inject(ServicesSnapshot { total: 0, services: vec![] });
        STORE.with(|s| {
            let st = s.borrow();
            assert_eq!(st.activity.len(), 2);
            assert_eq!(st.activity[1].kind, EventKind::Stopped);
        });
    }
}
