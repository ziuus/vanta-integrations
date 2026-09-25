use extism_pdk::*;
use serde::{Deserialize, Serialize};
use vanta_ext_sdk::{
    telemetry::query,
    ui::{unavailable, Block, Color, Line, Span, Style, Widget},
    API_VERSION_TELEMETRY,
};

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

#[plugin_fn]
pub fn metadata() -> FnResult<Vec<u8>> {
    Ok(vanta_ext_sdk::ExtensionMetadata::new(
        "filespace_queue",
        "FileSpace Queue Component",
        "0.1.0",
        "File operation queue component extension.",
        API_VERSION_TELEMETRY,
    )
    .to_json())
}

#[plugin_fn]
pub fn widgets(_: ()) -> FnResult<Vec<u8>> {
    let ids = serde_json::json!(["filespace_queue"]);
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

#[plugin_fn]
pub fn render_widget(widget_id: String) -> FnResult<Vec<u8>> {
    if widget_id != "filespace_queue" {
        return Ok(unavailable("UNKNOWN", "invalid widget").to_json());
    }

    let req = serde_json::json!({ "topic": "fs_ops" });
    if let Ok(val) = query(&serde_json::to_string(&req).unwrap()) {
        if let Ok(tasks) = serde_json::from_value::<Vec<TaskProgress>>(val) {
            let active: Vec<_> = tasks
                .into_iter()
                .filter(|t| t.status != "Complete")
                .collect();
            if active.is_empty() {
                let widget = Widget::paragraph(vec![Line::new(vec![Span {
                    content: "No operations running.".to_string(),
                    style: Some(Style::dim()),
                }])])
                .block(Block::titled(" OPERATION QUEUE "));
                return Ok(widget.to_json());
            }
            let mut lines = vec![];
            for task in active.iter().take(4) {
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
            let widget = Widget::paragraph(lines).block(Block::titled(" OPERATION QUEUE "));
            return Ok(widget.to_json());
        }
    }
    Ok(unavailable("QUEUE", "no telemetry").to_json())
}
