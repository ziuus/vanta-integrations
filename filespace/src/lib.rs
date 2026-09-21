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

#[derive(Clone, Default, Serialize, Deserialize, Debug)]
pub struct TaskProgress {
    pub id: String,
    pub operation: String,
    pub source: String,
    pub dest: String,
    pub progress: f64,
    pub bytes_processed: u64,
    pub total_bytes: u64,
    pub speed_bps: f64,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct State {
    path: String,
    selected: usize,
    query: String,
    filter: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            path: "/home/zius".to_string(), // Or root
            selected: 0,
            query: "".to_string(),
            filter: "ALL".to_string(),
        }
    }
}

// Extism static state workaround (since memory doesn't persist across calls automatically if we don't save it)
// We will use config or memory. But for simple widgets without plugin lifecycle, we can just load/save from memory.
fn load_state() -> State {
    if let Ok(Some(data)) = var::get::<Vec<u8>>("state") {
        if let Ok(s) = serde_json::from_slice(&data) {
            return s;
        }
    }
    State::default()
}

fn save_state(s: &State) {
    if let Ok(data) = serde_json::to_vec(s) {
        let _ = var::set("state", &data);
    }
}

#[plugin_fn]
pub fn metadata() -> FnResult<Vec<u8>> {
    Ok(vanta_ext_sdk::ExtensionMetadata::new(
        "filespace",
        "FileSpace",
        "0.1.0",
        "A premium terminal file management workspace.",
        API_VERSION_TELEMETRY,
    )
    .to_json())
}

#[plugin_fn]
pub fn widgets(_: ()) -> FnResult<Vec<u8>> {
    let ids = serde_json::json!([
        "filespace_browser",
        "filespace_sidebar",
        "filespace_preview",
        "filespace_queue",
        "filespace_path"
    ]);
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
    let req = serde_json::json!({
        "topic": "fs_list",
        "path": path
    });
    let val = query(&serde_json::to_string(&req).unwrap())?;
    serde_json::from_value(val).map_err(|_| TelemetryError::Decode("bad fs json".into()))
}

fn build_browser(_width: u16, height: u16) -> Widget {
    let state = load_state();
    let Ok(resp) = fetch_dir(&state.path) else {
        return unavailable("BROWSER", "no fs telemetry");
    };

    let mut lines = vec![];
    let max_lines = height.saturating_sub(2) as usize;

    // pagination / scrolling
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

    Widget::paragraph(lines).block(Block::titled(format!(
        " FILES · {} ITEMS ",
        resp.items.len()
    )))
}

fn build_path(_width: u16, _height: u16) -> Widget {
    let state = load_state();
    Widget::paragraph(vec![Line::new(vec![
        Span {
            content: " 📁 ".to_string(),
            style: Some(Style::fg(Color::CYAN)),
        },
        Span {
            content: state.path.clone(),
            style: Some(Style::fg(Color::WHITE).bold()),
        },
    ])])
    .block(Block::titled(" LOCATION ".to_string()))
}

fn build_sidebar(_width: u16, _height: u16) -> Widget {
    let mut lines = vec![];
    lines.push(Line::new(vec![Span {
        content: " NAVIGATION".to_string(),
        style: Some(Style::dim().bold()),
    }]));
    lines.push(Line::new(vec![Span {
        content: " 🏠 Home".to_string(),
        style: Some(Style::fg(Color::WHITE)),
    }]));
    lines.push(Line::new(vec![Span {
        content: " 💻 Desktop".to_string(),
        style: None,
    }]));
    lines.push(Line::new(vec![Span {
        content: " 📥 Downloads".to_string(),
        style: None,
    }]));
    lines.push(Line::new(vec![Span {
        content: " 📄 Documents".to_string(),
        style: None,
    }]));
    lines.push(Line::new(vec![Span {
        content: " 🖼  Pictures".to_string(),
        style: None,
    }]));

    lines.push(Line::new(vec![]));
    lines.push(Line::new(vec![Span {
        content: " BOOKMARKS".to_string(),
        style: Some(Style::dim().bold()),
    }]));
    lines.push(Line::new(vec![Span {
        content: " ★  Projects".to_string(),
        style: Some(Style::fg(Color::CYAN)),
    }]));

    Widget::paragraph(lines).block(Block::titled(" SIDEBAR ".to_string()))
}

fn build_preview(_width: u16, _height: u16) -> Widget {
    let state = load_state();
    let Ok(resp) = fetch_dir(&state.path) else {
        return unavailable("PREVIEW", "no telemetry");
    };

    if state.selected < resp.items.len() {
        let item = &resp.items[state.selected];
        let mut lines = vec![];
        lines.push(Line::new(vec![Span {
            content: item.name.clone(),
            style: Some(Style::fg(Color::WHITE).bold()),
        }]));
        lines.push(Line::new(vec![Span {
            content: format!("Path: {}", item.path),
            style: Some(Style::dim()),
        }]));
        lines.push(Line::new(vec![Span {
            content: format!("Size: {}", format_size(item.size)),
            style: Some(Style::dim()),
        }]));
        lines.push(Line::new(vec![Span {
            content: if item.is_dir {
                "Directory".into()
            } else {
                "File".into()
            },
            style: Some(Style::fg(Color::CYAN)),
        }]));

        Widget::paragraph(lines).block(Block::titled(" PREVIEW ".to_string()))
    } else {
        unavailable("PREVIEW", "Nothing selected")
    }
}

fn build_queue(_width: u16, height: u16) -> Widget {
    let req = serde_json::json!({ "topic": "fs_ops" });
    if let Ok(val) = query(&serde_json::to_string(&req).unwrap()) {
        if let Ok(tasks) = serde_json::from_value::<Vec<TaskProgress>>(val) {
            let active: Vec<_> = tasks
                .into_iter()
                .filter(|t| t.status != "Complete")
                .collect();
            if active.is_empty() {
                return Widget::paragraph(vec![Line::new(vec![Span {
                    content: "No operations running.".to_string(),
                    style: Some(Style::dim()),
                }])])
                .block(Block::titled(" OPERATION QUEUE ".to_string()));
            }
            let mut lines = vec![];
            for task in active.iter().take(height.saturating_sub(2) as usize) {
                lines.push(Line::new(vec![Span {
                    content: format!("{} {} -> {}", task.operation, task.source, task.dest),
                    style: Some(Style::fg(Color::WHITE)),
                }]));

                let bar_w: usize = 20;
                let filled = (task.progress * bar_w as f64) as usize;
                let empty = bar_w.saturating_sub(filled);
                let speed = format_size(task.speed_bps as u64);

                lines.push(Line::new(vec![
                    Span {
                        content: format!("{}●{} ", "━".repeat(filled), "─".repeat(empty)),
                        style: Some(Style::fg(Color::CYAN)),
                    },
                    Span {
                        content: format!("{:.0}% ({} /s)", task.progress * 100.0, speed),
                        style: Some(Style::dim()),
                    },
                ]));
            }
            return Widget::paragraph(lines).block(Block::titled(" OPERATION QUEUE ".to_string()));
        }
    }
    unavailable("QUEUE", "no telemetry")
}

#[plugin_fn]
pub fn render_widget(widget_id: String) -> FnResult<Vec<u8>> {
    let widget = match widget_id.as_str() {
        "filespace_browser" => build_browser(60, 20),
        "filespace_sidebar" => build_sidebar(20, 20),
        "filespace_preview" => build_preview(40, 20),
        "filespace_queue" => build_queue(40, 6),
        "filespace_path" => build_path(80, 3),
        _ => unavailable("UNKNOWN", "invalid widget"),
    };
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

        let mut state = load_state();
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
                // Delete action (Trash)
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
            "c" => {
                // We could implement "mark for copy" or just copy to a fixed target.
                // For a simple demo, copy a file to <name>.copy
                if state.selected < resp.items.len() {
                    let item = &resp.items[state.selected];
                    if !item.is_dir {
                        let dest = format!("{}.copy", item.path);
                        let req = serde_json::json!({
                            "topic": "fs_action",
                            "action": "copy",
                            "source": item.path,
                            "dest": dest
                        });
                        let _ = query::<serde_json::Value>(&serde_json::to_string(&req).unwrap());
                    }
                }
            }

            _ => handled = false,
        }

        if handled {
            save_state(&state);
            return Ok(serde_json::to_vec(&true).unwrap());
        }
    }

    Ok(serde_json::to_vec(&false).unwrap())
}
