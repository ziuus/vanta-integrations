#![allow(dead_code)]
use extism_pdk::*;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use vanta_ext_sdk::{
    telemetry::query,
    ui::{unavailable, Block, Color, Line, Span, Style, Widget},
    API_VERSION_TELEMETRY,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};

#[host_fn]
extern "ExtismHost" {
    fn vanta_query(input: String) -> String;
}

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

thread_local! {
    static CACHED_ART: RefCell<Option<(String, Vec<Line>)>> = RefCell::new(None);
}

fn fetch_data() -> Result<State, vanta_ext_sdk::telemetry::TelemetryError> {
    let req = serde_json::json!({ "topic": "media" });
    let val = query(&serde_json::to_string(&req).unwrap())?;
    serde_json::from_value(val).map_err(|_| vanta_ext_sdk::telemetry::TelemetryError::Decode("bad media json".into()))
}

fn get_active(state: &State) -> Option<Track> {
    state.all_tracks.iter().find(|t| t.is_active).cloned()
}

fn get_image_lines(url: &str) -> Vec<Line> {
    if url.is_empty() { return vec![]; }
    
    // Check cache
    let mut cached = None;
    CACHED_ART.with(|c| {
        if let Some((cached_url, lines)) = c.borrow().as_ref() {
            if cached_url == url {
                cached = Some(lines.clone());
            }
        }
    });
    if let Some(lines) = cached {
        return lines;
    }

    // Not cached, fetch it
    let mut bytes = Vec::new();
    if url.starts_with("http://") || url.starts_with("https://") {
        let req = extism_pdk::HttpRequest::new(url);
        if let Ok(res) = extism_pdk::http::request::<()>(&req, None) {
            bytes = res.body();
        }
    } else if url.starts_with("file://") {
        let path = url.strip_prefix("file://").unwrap();
        // url decode? mpris might percent encode
        let path = path.replace("%20", " ");
        let req = serde_json::json!({ "topic": "fs_read", "path": path });
        if let Ok(res) = unsafe { vanta_query(req.to_string()) } {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&res) {
                if let Some(b64) = val.get("data").and_then(|d| d.get("bytes_b64")).and_then(|b| b.as_str()) {
                    if let Ok(b) = STANDARD.decode(b64) {
                        bytes = b;
                    }
                }
            }
        }
    } else if url.starts_with("/") {
        let req = serde_json::json!({ "topic": "fs_read", "path": url });
        if let Ok(res) = unsafe { vanta_query(req.to_string()) } {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&res) {
                if let Some(b64) = val.get("data").and_then(|d| d.get("bytes_b64")).and_then(|b| b.as_str()) {
                    if let Ok(b) = STANDARD.decode(b64) {
                        bytes = b;
                    }
                }
            }
        }
    }

    if bytes.is_empty() {
        return vec![];
    }

    let mut lines = Vec::new();
    if let Ok(img) = image::load_from_memory(&bytes) {
        let img = img.to_rgb8();
        let target_w = 16;
        let target_h = 16; // 8 lines
        let thumb = image::imageops::thumbnail(&img, target_w, target_h);
        let (w, h) = thumb.dimensions();

        for y in (0..h).step_by(2) {
            let mut spans = Vec::new();
            for x in 0..w {
                let top = thumb.get_pixel(x, y);
                let bottom = if y + 1 < h {
                    Some(thumb.get_pixel(x, y + 1))
                } else {
                    None
                };

                let top_color = Color::Rgb(top[0], top[1], top[2]);
                let style = Style::fg(top_color);
                let mut style = style;
                if let Some(b) = bottom {
                    style.bg = Some(Color::Rgb(b[0], b[1], b[2]));
                }
                spans.push(Span { content: "▀".to_string(), style: Some(style) });
            }
            lines.push(Line::new(spans));
        }
    }

    CACHED_ART.with(|c| {
        *c.borrow_mut() = Some((url.to_string(), lines.clone()));
    });

    lines
}

#[plugin_fn]
pub fn metadata() -> FnResult<Vec<u8>> {
    Ok(vanta_ext_sdk::ExtensionMetadata::new(
        "mediadeck_now_playing",
        "MediaDeck Now_Playing Component",
        "0.1.0",
        "Now_Playing micro-extension with ASCII album art.",
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
        let art_lines = get_image_lines(&t.art_url);
        
        let mut final_lines = Vec::new();
        
        let mut title_spans = vec![];
        title_spans.push(Span { content: t.title.clone(), style: Some(Style::fg(Color::WHITE).bold()) });
        
        let mut sub_spans = vec![];
        let sub = if t.album.is_empty() { t.artist.clone() } else { format!("{} · {}", t.artist, t.album) };
        sub_spans.push(Span { content: sub, style: Some(Style::dim()) });

        // If we have art, we pad the text to sit next to it.
        // Wait, Vanta SDK Widget::paragraph doesn't support layout columns natively in WASM yet.
        // We have to build it manually line by line!
        
        if art_lines.is_empty() {
            final_lines.push(Line::new(title_spans));
            final_lines.push(Line::new(sub_spans));
        } else {
            // we have ~8 lines of art
            for (i, art_line) in art_lines.into_iter().enumerate() {
                let mut row_spans = art_line.spans;
                row_spans.push(Span { content: "  ".to_string(), style: None });
                
                if i == 0 {
                    row_spans.extend(title_spans.clone());
                } else if i == 1 {
                    row_spans.extend(sub_spans.clone());
                } else if i == 3 {
                    row_spans.push(Span { content: format!("status: {}", t.status), style: Some(Style::dim()) });
                } else if i == 4 {
                    row_spans.push(Span { content: format!("vol: {}%", (t.volume * 100.0) as u32), style: Some(Style::dim()) });
                }
                
                final_lines.push(Line::new(row_spans));
            }
        }

        let w = Widget::paragraph(final_lines).block(Block::titled(" NOW PLAYING "));
        Ok(w.to_json())
    } else {
        Ok(unavailable("MEDIA", "No active track").to_json())
    }
}
