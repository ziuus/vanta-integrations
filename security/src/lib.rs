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
        id: "security".to_string(),
        name: "Vanta Security Pack".to_string(),
        author: "Community".to_string(),
        version: "1.0.0".to_string(),
        description: "Security monitoring components.".to_string(),
        api_version: "0.9.0".to_string(),
    };
    Ok(serde_json::to_vec(&meta)?)
}

#[plugin_fn]
pub fn widgets() -> FnResult<Vec<u8>> {
    let widgets = vec!["cve_feed"];
    Ok(serde_json::to_vec(&widgets)?)
}

#[plugin_fn]
pub fn render_widget(widget_id: String) -> FnResult<Vec<u8>> {
    if widget_id == "cve_feed" {
        // Build a mock list of CVEs for the UI
        let ui = json!({
            "type": "List",
            "block": {
                "title": " 🛡️ Live CVE Feed ",
                "bordered": true,
                "border_color": "red"
            },
            "items": [
                { "spans": [ { "content": "[CRIT] ", "style": { "fg": "red", "bold": true } }, { "content": "CVE-2024-3094 (xz backdoor)" } ] },
                { "spans": [ { "content": "[HIGH] ", "style": { "fg": "yellow", "bold": true } }, { "content": "CVE-2024-21626 (runc breakout)" } ] },
                { "spans": [ { "content": "[WARN] ", "style": { "fg": "gray" } }, { "content": "CVE-2023-38545 (curl heap overflow)" } ] }
            ]
        });
        Ok(serde_json::to_vec(&ui)?)
    } else {
        let err = json!({
            "type": "Paragraph",
            "lines": [ { "spans": [ { "content": "Unknown widget ID" } ] } ],
            "wrap": true
        });
        Ok(serde_json::to_vec(&err)?)
    }
}
