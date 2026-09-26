use extism_pdk::*;
use vanta_ext_sdk::{
    ui::{Block, Color, Line, Span, Style, Widget},
    API_VERSION_TELEMETRY,
};

#[plugin_fn]
pub fn metadata() -> FnResult<Vec<u8>> {
    Ok(vanta_ext_sdk::ExtensionMetadata::new(
        "filespace_sidebar",
        "FileSpace Sidebar Component",
        "0.1.0",
        "File sidebar component extension.",
        API_VERSION_TELEMETRY,
    )
    .to_json())
}

#[plugin_fn]
pub fn widgets(_: ()) -> FnResult<Vec<u8>> {
    let ids = serde_json::json!(["filespace_sidebar"]);
    Ok(serde_json::to_vec(&ids).unwrap_or_default())
}

#[plugin_fn]
pub fn render_widget(widget_id: String) -> FnResult<Vec<u8>> {
    if widget_id != "filespace_sidebar" {
        return Ok(vanta_ext_sdk::ui::unavailable("UNKNOWN", "invalid widget").to_json());
    }
#[allow(clippy::vec_init_then_push)]

    let mut lines = vec![
    Line::new(vec![Span {
        content: " NAVIGATION".to_string(),
        style: Some(Style::dim().bold()),
    }]),
];
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

    let widget = Widget::paragraph(lines).block(Block::titled(" SIDEBAR "));
    Ok(widget.to_json())
}
