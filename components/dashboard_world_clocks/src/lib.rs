use chrono::{Utc, TimeZone};
use chrono_tz::Tz;
use extism_pdk::*;
use std::str::FromStr;
use vanta_ext_sdk::{
    ui, Block, Color, ExtensionMetadata, Line, Span, Style, Widget, API_VERSION_BASE,
};

#[plugin_fn]
pub fn metadata() -> FnResult<Vec<u8>> {
    let meta = ExtensionMetadata::new(
        "dashboard_world_clocks",
        "World Clocks",
        "1.1.0",
        "Robust global timezones with day/night indicators.",
        API_VERSION_BASE,
    );
    Ok(meta.to_json())
}

#[plugin_fn]
pub fn widgets(_: ()) -> FnResult<Vec<u8>> {
    let ids = serde_json::json!(["world_clocks"]);
    Ok(serde_json::to_vec(&ids).unwrap_or_default())
}

#[plugin_fn]
pub fn render_widget(id: String) -> FnResult<Vec<u8>> {
    if id != "world_clocks" {
        return Ok(ui::unavailable("UNKNOWN", "invalid widget").to_json());
    }

    let zones = [
        ("SFO", "America/Los_Angeles"),
        ("NYC", "America/New_York"),
        ("LON", "Europe/London"),
        ("DXB", "Asia/Dubai"),
        ("TOK", "Asia/Tokyo"),
        ("SYD", "Australia/Sydney"),
    ];

    let now = Utc::now();
    let mut spans = Vec::new();

    for (i, (code, tz_name)) in zones.iter().enumerate() {
        let tz = Tz::from_str(tz_name).unwrap();
        let local_time = now.with_timezone(&tz);
        
        let hour = local_time.format("%H").to_string().parse::<u32>().unwrap_or(0);
        
        // Day/night indicator
        let (icon, color) = match hour {
            5..=7 => ("🌅", Color::Rgb(255, 165, 0)),
            8..=16 => ("☀️", Color::Rgb(255, 223, 0)),
            17..=19 => ("🌇", Color::Rgb(255, 69, 0)),
            _ => ("🌙", Color::Rgb(100, 149, 237)),
        };

        spans.push(ui::span(format!(" {} ", icon), Style::fg(color)));
        spans.push(ui::span(format!("{} ", code), Style::fg(Color::GRAY)));
        spans.push(ui::span(
            local_time.format("%H:%M").to_string(),
            Style::fg(Color::WHITE).bold(),
        ));

        if i < zones.len() - 1 {
            spans.push(ui::span("  │", Style::fg(Color::DARK_GRAY)));
        }
    }

    let widget = Widget::paragraph(vec![Line::new(spans)]).block(
        Block::titled(" 🌍 Global Pulse ".to_string())
    );

    Ok(widget.to_json())
}
