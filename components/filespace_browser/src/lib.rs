use extism_pdk::*;
use serde::{Deserialize, Serialize};
use vanta_ext_sdk::{
    telemetry::{query, TelemetryError},
    ui::{unavailable, Block, Color, Line, Span, Style, Widget},
    API_VERSION_TELEMETRY,
};

#[derive(Clone, Default, Serialize, Deserialize, Debug)]
pub struct FileItem {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub modified: u64,
}

#[derive(Clone, Default, Serialize, Deserialize, Debug)]
pub struct FsListResponse {
    pub current_dir: String,
    pub parent: Option<String>,
    pub items: Vec<FileItem>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SharedState {
    pub path: String,
    pub selected: usize,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            path: "/home/zius".to_string(),
            selected: 0,
        }
    }
}

fn get_shared_state() -> SharedState {
    let req = serde_json::json!({ "topic": "state_get", "key": "filespace_state" });
    if let Ok(val) = query::<serde_json::Value>(&serde_json::to_string(&req).unwrap()) {
        if let Some(v) = val.get("value") {
            if let Ok(s) = serde_json::from_value::<SharedState>(v.clone()) {
                return s;
            }
        }
    }
    SharedState::default()
}

fn set_shared_state(s: &SharedState) {
    let req = serde_json::json!({
        "topic": "state_set",
        "key": "filespace_state",
        "value": s
    });
    let _ = query::<serde_json::Value>(&serde_json::to_string(&req).unwrap());
}

#[plugin_fn]
pub fn metadata() -> FnResult<Vec<u8>> {
    Ok(vanta_ext_sdk::ExtensionMetadata::new(
        "filespace_browser",
        "FileSpace Browser Component",
        "0.1.0",
        "File browser component extension.",
        API_VERSION_TELEMETRY,
    )
    .to_json())
}

#[plugin_fn]
pub fn widgets(_: ()) -> FnResult<Vec<u8>> {
    let ids = serde_json::json!(["filespace_browser"]);
    Ok(serde_json::to_vec(&ids).unwrap_or_default())
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

fn fetch_dir(path: &str) -> Result<FsListResponse, TelemetryError> {
    let req = serde_json::json!({ "topic": "fs_list", "path": path });
    let val = query(&serde_json::to_string(&req).unwrap())?;
    serde_json::from_value(val).map_err(|_| TelemetryError::Decode("bad fs json".into()))
}

#[plugin_fn]
pub fn render_widget(widget_id: String) -> FnResult<Vec<u8>> {
    if widget_id != "filespace_browser" {
        return Ok(unavailable("UNKNOWN", "invalid widget").to_json());
    }

    let state = get_shared_state();
    let Ok(resp) = fetch_dir(&state.path) else {
        return Ok(unavailable("BROWSER", "no fs telemetry").to_json());
    };

    let mut lines = vec![];
    let max_lines = 18;
    let start_idx = if state.selected >= max_lines {
        state.selected - max_lines + 1
    } else {
        0
    };

    for (i, item) in resp
        .items
        .iter()
        .enumerate()
        .skip(start_idx)
        .take(max_lines)
    {
        let is_selected = i == state.selected;
        let prefix = if is_selected { "▶" } else { " " };
        let style = if is_selected {
            Some(Style::fg(Color::CYAN).bold())
        } else {
            None
        };

        let icon = if item.is_dir { "📁" } else { "📄" };
        let mut name = item.name.clone();
        if name.len() > 30 {
            name.truncate(27);
            name.push_str("...");
        }

        let size_str = if item.is_dir {
            "DIR".to_string()
        } else {
            format_size(item.size)
        };

        lines.push(Line::new(vec![
            Span {
                content: format!("{} {} {:<30}", prefix, icon, name),
                style,
            },
            Span {
                content: format!("{:>10}", size_str),
                style: Some(Style::dim()),
            },
        ]));
    }

    let widget = Widget::paragraph(lines).block(Block::titled(format!(
        " FILES · {} ITEMS ",
        resp.items.len()
    )));
    Ok(widget.to_json())
}

#[derive(Deserialize)]
struct KeyPayload {
    widget: String,
    key: String,
}

#[plugin_fn]
pub fn handle_key(payload_str: String) -> FnResult<Vec<u8>> {
    if let Ok(payload) = serde_json::from_str::<KeyPayload>(&payload_str) {
        if payload.widget != "filespace_browser" {
            return Ok(serde_json::to_vec(&false).unwrap());
        }

        let mut state = get_shared_state();
        let Ok(resp) = fetch_dir(&state.path) else {
            return Ok(serde_json::to_vec(&false).unwrap());
        };

        let mut handled = true;
        match payload.key.as_str() {
            "up" => {
                if state.selected > 0 {
                    state.selected -= 1;
                }
            }
            "down" => {
                if state.selected + 1 < resp.items.len() {
                    state.selected += 1;
                }
            }
            "enter" => {
                if state.selected < resp.items.len() {
                    let item = &resp.items[state.selected];
                    if item.is_dir {
                        state.path = item.path.clone();
                        state.selected = 0;
                    }
                }
            }
            "backspace" => {
                if let Some(parent) = resp.parent {
                    state.path = parent;
                    state.selected = 0;
                }
            }
            "d" => {
                if state.selected < resp.items.len() {
                    let item = &resp.items[state.selected];
                    let req = serde_json::json!({
                        "topic": "fs_action",
                        "action": "trash",
                        "source": item.path
                    });
                    let _ = query::<serde_json::Value>(&serde_json::to_string(&req).unwrap());
                }
            }
            _ => handled = false,
        }

        if handled {
            set_shared_state(&state);
            return Ok(serde_json::to_vec(&true).unwrap());
        }
    }

    Ok(serde_json::to_vec(&false).unwrap())
}
