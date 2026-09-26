#![allow(dead_code)]
use extism_pdk::*;
use serde::{Deserialize, Serialize};
use vanta_ext_sdk::{
    telemetry::query,
    ui::{unavailable, Block, Color, Line, Span, Style, Widget},
    API_VERSION_TELEMETRY,
};

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Track {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art_url: String,
    pub length_ms: u64,
    pub position_ms: u64,
    pub is_active: bool,
    pub player_name: String,
    pub volume: f64,
    pub status: String,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct State {
    pub all_tracks: Vec<Track>,
}

fn fetch_data() -> Result<State, vanta_ext_sdk::telemetry::TelemetryError> {
    let req = serde_json::json!({ "topic": "media" });
    let val = query(&serde_json::to_string(&req).unwrap())?;
    serde_json::from_value(val).map_err(|_| vanta_ext_sdk::telemetry::TelemetryError::Decode("bad media json".into()))
}

fn get_active(state: &State) -> Option<Track> {
    state.all_tracks.iter().find(|t| t.is_active).cloned()
}

fn format_dur(ms: u64) -> String {
    let total_s = ms / 1000;
    let m = total_s / 60;
    let s = total_s % 60;
    if m > 59 {
        let h = m / 60;
        let m = m % 60;
        format!("{:02}:{:02}:{:02}", h, m, s)
    } else {
        format!("{:02}:{:02}", m, s)
    }
}


#[plugin_fn]
pub fn metadata() -> FnResult<Vec<u8>> {
    Ok(vanta_ext_sdk::ExtensionMetadata::new(
        "mediadeck_now_playing",
        "MediaDeck Now_Playing Component",
        "0.1.0",
        "Now_Playing micro-extension.",
        API_VERSION_TELEMETRY,
    )
    .to_json())
}

#[plugin_fn]
pub fn widgets(_: ()) -> FnResult<Vec<u8>> {
    let ids = serde_json::json!(["media_now_playing"]);
    Ok(serde_json::to_vec(&ids).unwrap_or_default())
}

#[plugin_fn]
pub fn render_widget(id: String) -> FnResult<Vec<u8>> {
    if id != "media_now_playing" { return Ok(unavailable("?", "?").to_json()); }
    let Ok(state) = fetch_data() else { return Ok(unavailable("MEDIA", "No telemetry").to_json()); };
    if let Some(t) = get_active(&state) {
        let w = Widget::paragraph(vec![
            Line::new(vec![Span { content: t.title, style: Some(Style::fg(Color::WHITE).bold()) }]),
            Line::new(vec![Span { content: format!("{} - {}", t.artist, t.album), style: Some(Style::dim()) }]),
        ]).block(Block::titled(" NOW PLAYING "));
        Ok(w.to_json())
    } else {
        Ok(unavailable("MEDIA", "No active track").to_json())
    }
}
