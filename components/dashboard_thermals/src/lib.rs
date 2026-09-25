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
        id: "dashboard_thermals".to_string(),
        name: "Radial Thermals".to_string(),
        author: "zius".to_string(),
        version: "1.0.0".to_string(),
        description: "Temperature gauges for CPU and GPU.".to_string(),
        api_version: "0.10.0".to_string(),
    };
    Ok(serde_json::to_vec(&meta)?)
}

#[plugin_fn]
pub fn widgets() -> FnResult<Vec<u8>> {
    let widgets = vec!["thermals"];
    Ok(serde_json::to_vec(&widgets)?)
}

#[extism_pdk::host_fn]
extern "ExtismHost" {
    fn vanta_query(input: String) -> String;
}

fn get_cpu_temp() -> f64 {
    if let Ok(res) = unsafe { vanta_query(r#"{"topic":"cpu"}"#.to_string()) } {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&res) {
            if let Some(max_temp) = val.get("data").and_then(|d| d.get("max_temp_c")).and_then(|v| v.as_f64()) {
                return max_temp;
            }
        }
    }
    40.0
}

fn get_gpu_temp() -> f64 {
    if let Ok(res) = unsafe { vanta_query(r#"{"topic":"gpu"}"#.to_string()) } {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&res) {
            if let Some(temp) = val.get("data").and_then(|d| d.get("temp_c")).and_then(|v| v.as_f64()) {
                return temp;
            }
        }
    }
    40.0
}

#[plugin_fn]
pub fn render_widget(widget_id: String) -> FnResult<Vec<u8>> {
    if widget_id == "thermals" {
        let cpu_t = get_cpu_temp();
        let gpu_t = get_gpu_temp();

        let cpu_color = if cpu_t > 80.0 { "red" } else if cpu_t > 60.0 { "yellow" } else { "green" };
        let gpu_color = if gpu_t > 80.0 { "red" } else if gpu_t > 60.0 { "yellow" } else { "cyan" };

        let ui = json!({
            "type": "Row",
            "children": [
                {
                    "type": "Gauge",
                    "ratio": (cpu_t / 100.0).clamp(0.0, 1.0),
                    "label": format!("{:.1}°C", cpu_t),
                    "color": cpu_color,
                    "block": { "title": "CPU Temp", "bordered": true }
                },
                {
                    "type": "Gauge",
                    "ratio": (gpu_t / 100.0).clamp(0.0, 1.0),
                    "label": format!("{:.1}°C", gpu_t),
                    "color": gpu_color,
                    "block": { "title": "GPU Temp", "bordered": true }
                }
            ]
        });
        Ok(serde_json::to_vec(&ui)?)
    } else {
        Ok(serde_json::to_vec(&json!({
            "type": "Paragraph",
            "lines": [ { "spans": [ { "content": "Unknown widget" } ] } ]
        }))?)
    }
}
