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
        id: "dashboard_github".to_string(),
        name: "GitHub Contributions".to_string(),
        author: "zius".to_string(),
        version: "1.0.0".to_string(),
        description: "Your GitHub green squares grid.".to_string(),
        api_version: "0.10.0".to_string(),
    };
    Ok(serde_json::to_vec(&meta)?)
}

#[plugin_fn]
pub fn widgets() -> FnResult<Vec<u8>> {
    let widgets = vec!["github"];
    Ok(serde_json::to_vec(&widgets)?)
}

#[extism_pdk::host_fn]
extern "ExtismHost" {
    fn vanta_query(input: String) -> String;
}

#[plugin_fn]
pub fn render_widget(widget_id: String) -> FnResult<Vec<u8>> {
    if widget_id == "github" {
        let mut total = 0;
        let mut user = String::new();
        
        if let Ok(res) = unsafe { vanta_query(r#"{"topic":"github"}"#.to_string()) } {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&res) {
                if let Some(data) = val.get("data") {
                    if !data.is_null() {
                        total = data.get("total_contributions").and_then(|v| v.as_u64()).unwrap_or(0);
                        user = data.get("username").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    }
                }
            }
        }

        let title = if user.is_empty() { " 🐙 GitHub ".to_string() } else { format!(" 🐙 {} ", user) };
        let ui = json!({
            "type": "Paragraph",
            "lines": [
                { "spans": [ { "content": format!("Total Contributions: {}", total), "style": { "fg": "green", "bold": true } } ] },
                { "spans": [ { "content": "(Run 'gh auth login' on host if 0)", "style": { "fg": "dark_gray" } } ] }
            ],
            "wrap": false,
            "block": {
                "title": title,
                "bordered": true,
                "border_color": "green"
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
