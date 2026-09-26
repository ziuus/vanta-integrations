use extism_pdk::*;
use vanta_ext_sdk::{ui, FnResult, Widget};
use ratatui::{
    style::{Color, Style},
    text::Line,
    widgets::Block,
};

// ── Dummy State / Storage ─────────────────────────────────────────────────────

// If your extension needs to hold state between render frames, you can use a thread-local
// or static Mutex (if the target supports it, though wasm32-unknown-unknown usually doesn't).
// For simple widgets, rendering statelessly by querying the host is best.

// ── Widget Builders ───────────────────────────────────────────────────────────

fn build_my_widget(_w: u16, _h: u16) -> Widget {
    let mut lines = Vec::new();
    
    lines.push(Line::new(vec![
        ui::span("Hello from ", Style::default()),
        ui::span("WASM!", Style::fg(Color::GREEN).bold()),
    ]));
    
    lines.push(Line::text("Edit src/lib.rs to change this.", Style::dim()));

    Widget::paragraph(lines).block(Block::titled(" MY EXTENSION ".to_string()))
}

// ── Plugin exports ────────────────────────────────────────────────────────────

#[plugin_fn]
pub fn metadata() -> FnResult<Vec<u8>> {
    Ok(vanta_ext_sdk::ExtensionMetadata::new(
        "my_vanta_extension",
        "My Extension",
        "0.1.0",
        "A custom WASM micro-extension",
        vanta_ext_sdk::API_VERSION_TELEMETRY,
    )
    .to_json())
}

#[plugin_fn]
pub fn widgets(_: ()) -> FnResult<Vec<u8>> {
    // Return an array of widget IDs that this plugin provides.
    let ids = serde_json::json!(["my_widget"]);
    Ok(serde_json::to_vec(&ids).unwrap_or_default())
}

#[plugin_fn]
pub fn render_widget(id: String) -> FnResult<Vec<u8>> {
    let widget = match id.as_str() {
        "my_widget" => build_my_widget(80, 24),
        _ => return Ok(vanta_ext_sdk::ui::unavailable("UNKNOWN", "invalid widget").to_json()),
    };
    Ok(widget.to_json())
}
