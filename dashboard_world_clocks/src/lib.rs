use chrono::{TimeZone, Utc};
use extism_pdk::*;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ExtensionMetadata {
    pub id: String,
    pub name: String,
    pub author: String,
    pub version: String,
    pub description: String,
    pub api_version: String,
}

#[plugin_fn]
pub fn metadata() -> FnResult<Vec<u8>> {
    let meta = ExtensionMetadata {
        id: "dashboard_world_clocks".to_string(),
        name: "World Clocks".to_string(),
        author: "zius".to_string(),
        version: "1.0.0".to_string(),
        description: "A strip showing the local time in multiple time zones.".to_string(),
        api_version: "0.10.0".to_string(),
    };
    Ok(serde_json::to_vec(&meta)?)
}

#[plugin_fn]
pub fn widgets() -> FnResult<Vec<u8>> {
    let widgets = vec!["world_clocks"];
    Ok(serde_json::to_vec(&widgets)?)
}

#[plugin_fn]
pub fn render_widget(widget_id: String) -> FnResult<Vec<u8>> {
    if widget_id == "world_clocks" {
        // Just hardcode some for now
        let zones = vec![
            ("SFO", -8),
            ("NYC", -5),
            ("LON", 0),
            ("TOK", 9),
        ];

        let now = Utc::now();
        let mut spans = Vec::new();

        for (i, (name, offset)) in zones.iter().enumerate() {
            let local_time = now + chrono::Duration::hours(*offset);
            spans.push(json!({
                "content": format!(" {} ", name),
                "style": { "fg": "gray" }
            }));
            spans.push(json!({
                "content": format!("{} ", local_time.format("%H:%M")),
                "style": { "fg": "white", "bold": true }
            }));
            if i < zones.len() - 1 {
                spans.push(json!({
                    "content": "·",
                    "style": { "fg": "dark_gray" }
                }));
            }
        }

        let ui = json!({
            "type": "Paragraph",
            "lines": [ { "spans": spans } ],
            "wrap": false,
            "block": {
                "title": " 🌍 World Clocks ",
                "bordered": true,
                "border_color": "cyan"
            }
        });
        Ok(serde_json::to_vec(&ui)?)
    } else {
        Ok(serde_json::to_vec(&json!({
            "type": "Paragraph",
            "lines": [ { "spans": [ { "content": "Unknown widget" } ] } ]
        }))?)
    }
}
