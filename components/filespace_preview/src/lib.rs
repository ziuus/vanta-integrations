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

#[plugin_fn]
pub fn metadata() -> FnResult<Vec<u8>> {
    Ok(vanta_ext_sdk::ExtensionMetadata::new(
        "filespace_preview",
        "FileSpace Preview Component",
        "0.1.0",
        "File preview component extension.",
        API_VERSION_TELEMETRY,
    )
    .to_json())
}

#[plugin_fn]
pub fn widgets(_: ()) -> FnResult<Vec<u8>> {
    let ids = serde_json::json!(["filespace_preview"]);
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
    if widget_id != "filespace_preview" {
        return Ok(unavailable("UNKNOWN", "invalid widget").to_json());
    }

    let state = get_shared_state();
    let Ok(resp) = fetch_dir(&state.path) else {
        return Ok(unavailable("PREVIEW", "no fs telemetry").to_json());
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
                "Directory".to_string()
            } else {
                "File".to_string()
            },
            style: Some(Style::fg(Color::CYAN)),
        }]));

        let widget = Widget::paragraph(lines).block(Block::titled(" PREVIEW "));
        Ok(widget.to_json())
    } else {
        Ok(unavailable("PREVIEW", "Nothing selected").to_json())
    }
}
