use extism_pdk::*;
use serde::{Deserialize, Serialize};
use vanta_ext_sdk::{
    telemetry::query,
    ui::{unavailable, Block, Color, Line, Span, Style, Widget},
    API_VERSION_TELEMETRY,
};

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
        "filespace_path",
        "FileSpace Path Component",
        "0.1.0",
        "File path breadcrumb component extension.",
        API_VERSION_TELEMETRY,
    )
    .to_json())
}

#[plugin_fn]
pub fn widgets(_: ()) -> FnResult<Vec<u8>> {
    let ids = serde_json::json!(["filespace_path"]);
    Ok(serde_json::to_vec(&ids).unwrap_or_default())
}

#[plugin_fn]
pub fn render_widget(widget_id: String) -> FnResult<Vec<u8>> {
    if widget_id != "filespace_path" {
        return Ok(unavailable("UNKNOWN", "invalid widget").to_json());
    }

    let state = get_shared_state();
    let widget = Widget::paragraph(vec![Line::new(vec![
        Span {
            content: " 📁 ".to_string(),
            style: Some(Style::fg(Color::CYAN)),
        },
        Span {
            content: state.path,
            style: Some(Style::fg(Color::WHITE).bold()),
        },
    ])])
    .block(Block::titled(" LOCATION "));

    Ok(widget.to_json())
}
