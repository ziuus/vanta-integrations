use extism_pdk::*;
use serde::{Deserialize, Serialize};
use vanta_ext_sdk::{
    telemetry::{query, TelemetryError},
    ui::{unavailable, Block, Color, Line, Span, Style, Widget},
    API_VERSION_TELEMETRY,
};

#[derive(Clone, Default, Serialize, Deserialize, Debug)]
pub struct MediaTrack {
    pub player: String,
    pub bus_name: String,
    pub status: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub length_ms: u64,
    pub position_ms: u64,
    pub volume: Option<f64>,
    pub art_url: String,
}

#[derive(Clone, Default, Serialize, Deserialize, Debug)]
pub struct MediaSnapshot {
    pub players: Vec<MediaTrack>,
    pub active: Option<String>,
}

#[derive(Deserialize)]
struct KeyPayload {
    widget: String,
    key: String,
}

#[plugin_fn]
pub fn metadata() -> FnResult<Vec<u8>> {
    Ok(vanta_ext_sdk::ExtensionMetadata::new(
        "mediadeck",
        "MediaDeck",
        "0.1.0",
        "Full terminal media center.",
        API_VERSION_TELEMETRY,
    )
    .to_json())
}

#[plugin_fn]
pub fn widgets(_: ()) -> FnResult<Vec<u8>> {
    let ids = serde_json::json!([
        "media_now_playing",
        "media_visualizer",
        "media_queue",
        "media_players",
        "media_signal",
        "media_transport"
    ]);
    Ok(serde_json::to_vec(&ids).unwrap_or_default())
}

fn fetch_data() -> Result<MediaSnapshot, TelemetryError> {
    let val = query("media")?;
    serde_json::from_value(val).map_err(|_| TelemetryError::Decode("bad media json".to_string()))
}

fn get_active(snap: &MediaSnapshot) -> Option<&MediaTrack> {
    if let Some(active) = &snap.active {
        snap.players.iter().find(|p| p.bus_name == *active)
    } else {
        None
    }
}

fn format_dur(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

fn build_queue(_width: u16, _height: u16) -> Widget {
    unavailable(
        "QUEUE",
        "MPRIS TrackList interface unsupported by current player",
    )
}
fn build_now_playing(_width: u16, height: u16) -> Widget {
    let Ok(snap) = fetch_data() else {
        return unavailable("NOW PLAYING", "no telemetry");
    };
    let Some(track) = get_active(&snap) else {
        return unavailable("NOW PLAYING", "No compatible media session detected.");
    };

    let mut lines = vec![];

    lines.push(Line::new(vec![Span {
        content: track.title.clone(),
        style: Some(Style::fg(Color::WHITE).bold()),
    }]));
    lines.push(Line::new(vec![Span {
        content: track.artist.clone(),
        style: Some(Style::fg(Color::GRAY)),
    }]));
    lines.push(Line::new(vec![Span {
        content: track.album.clone(),
        style: Some(Style::dim()),
    }]));

    // add padding
    for _ in 0..height.saturating_sub(6) {
        lines.push(Line::new(vec![]));
    }

    // Seek bar
    let pos_s = format_dur(track.position_ms);
    let len_s = format_dur(track.length_ms);
    let ratio = if track.length_ms > 0 {
        (track.position_ms as f64 / track.length_ms as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let bar_width: usize = 30;
    let filled = (ratio * bar_width as f64).round() as usize;
    let empty = bar_width.saturating_sub(filled);

    let bar = format!("{}●{}", "━".repeat(filled), "─".repeat(empty));

    lines.push(Line::new(vec![
        Span {
            content: format!("{:<7}", pos_s),
            style: Some(Style::dim()),
        },
        Span {
            content: bar,
            style: Some(Style::fg(Color::CYAN)),
        },
        Span {
            content: format!("{:>7}", len_s),
            style: Some(Style::dim()),
        },
    ]));

    Widget::paragraph(lines).block(Block::titled(format!(" NOW PLAYING · {} ", track.player)))
}

fn build_visualizer(_width: u16, height: u16) -> Widget {
    let Ok(snap) = fetch_data() else {
        return unavailable("VISUALIZER", "no telemetry");
    };
    let Some(track) = get_active(&snap) else {
        return unavailable("VISUALIZER", "no media");
    };

    let mut lines = vec![];
    let h = height.saturating_sub(2);

    let chars = [" ", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
    let num_bars = 40;

    let offset = if track.status == "Playing" {
        track.position_ms / 100
    } else {
        0
    };

    for row in 0..h {
        let mut row_str = String::new();
        for col in 0..num_bars {
            // deterministic pseudo-random height based on track name hash, col, and offset
            let seed = track.title.len() as u64 + col as u64;
            let val =
                ((seed.wrapping_mul(1103515245).wrapping_add(12345)) ^ offset) % (h as u64 + 4);

            if row as u64 + val >= h as u64 {
                let char_idx = (val % 8) as usize;
                row_str.push_str(chars[char_idx]);
            } else {
                row_str.push_str(" ");
            }
        }
        lines.push(Line::new(vec![Span {
            content: row_str,
            style: Some(Style::fg(Color::CYAN)),
        }]));
    }

    Widget::paragraph(lines).block(Block::titled(" AUDIO ACTIVITY "))
}

fn build_players(_width: u16, _height: u16) -> Widget {
    let Ok(snap) = fetch_data() else {
        return unavailable("PLAYERS", "no telemetry");
    };
    if snap.players.is_empty() {
        return unavailable("PLAYERS", "No compatible media session detected.");
    }

    let mut lines = vec![];
    for p in &snap.players {
        let is_active = snap.active.as_ref() == Some(&p.bus_name);
        let prefix = if is_active { "●" } else { "○" };
        let color = if is_active { Color::CYAN } else { Color::GRAY };

        lines.push(Line::new(vec![
            Span {
                content: format!("{} {} ", prefix, p.player),
                style: Some(Style::fg(color.clone()).bold()),
            },
            Span {
                content: format!("· {}", p.status),
                style: Some(Style::fg(color.clone())),
            },
        ]));
    }

    Widget::paragraph(lines).block(Block::titled(" PLAYER / OUTPUTS "))
}

fn build_signal(_width: u16, _height: u16) -> Widget {
    let Ok(snap) = fetch_data() else {
        return unavailable("MEDIA SIGNAL", "no telemetry");
    };
    let Some(track) = get_active(&snap) else {
        return unavailable("MEDIA SIGNAL", "no media");
    };

    let mut lines = vec![];

    lines.push(Line::new(vec![Span {
        content: track.status.to_uppercase(),
        style: Some(Style::fg(Color::CYAN).bold()),
    }]));

    let vol_pct = (track.volume.unwrap_or(1.0) * 100.0) as u32;
    lines.push(Line::new(vec![Span {
        content: format!("VOLUME: {}%", vol_pct),
        style: Some(Style::dim()),
    }]));

    let pos_s = format_dur(track.position_ms);
    let len_s = format_dur(track.length_ms);
    lines.push(Line::new(vec![Span {
        content: format!("{} / {}", pos_s, len_s),
        style: Some(Style::fg(Color::WHITE)),
    }]));

    Widget::paragraph(lines).block(Block::titled(" SIGNAL "))
}

fn build_transport(_width: u16, _height: u16) -> Widget {
    let mut lines = vec![];

    lines.push(Line::new(vec![
        Span {
            content: " ◀ ".to_string(),
            style: Some(Style::fg(Color::WHITE)),
        },
        Span {
            content: "(P)rev ".to_string(),
            style: Some(Style::dim()),
        },
        Span {
            content: " ▶ ".to_string(),
            style: Some(Style::fg(Color::CYAN).bold()),
        },
        Span {
            content: "(Space) ".to_string(),
            style: Some(Style::dim()),
        },
        Span {
            content: " ▶▶ ".to_string(),
            style: Some(Style::fg(Color::WHITE)),
        },
        Span {
            content: "(N)ext ".to_string(),
            style: Some(Style::dim()),
        },
        Span {
            content: "  🔊 ".to_string(),
            style: Some(Style::fg(Color::WHITE)),
        },
        Span {
            content: "(< / >) ".to_string(),
            style: Some(Style::dim()),
        },
    ]));

    Widget::paragraph(lines).block(Block::titled(" CONTROLS "))
}

#[plugin_fn]
pub fn render_widget(widget_id: String) -> FnResult<Vec<u8>> {
    let widget = match widget_id.as_str() {
        "media_now_playing" => build_now_playing(60, 10),
        "media_queue" => build_queue(30, 10),
        "media_visualizer" => build_visualizer(40, 10),
        "media_players" => build_players(30, 8),
        "media_signal" => build_signal(30, 8),
        "media_transport" => build_transport(60, 3),
        _ => unavailable("UNKNOWN", "invalid widget"),
    };
    Ok(widget.to_json())
}

#[plugin_fn]
pub fn handle_key(payload_str: String) -> FnResult<Vec<u8>> {
    if let Ok(payload) = serde_json::from_str::<KeyPayload>(&payload_str) {
        let action = match payload.key.as_str() {
            "space" => "play_pause",
            "p" => "previous",
            "n" => "next",
            "<" => "volume_down",
            ">" => "volume_up",
            _ => return Ok(serde_json::to_vec(&false).unwrap()),
        };

        // Ensure only one widget handles the key per broadcast to avoid duplicate DBus calls
        if payload.widget != "media_now_playing" {
            // Other widgets will just say "yes we handled it" but do nothing,
            // OR they can return false and let media_now_playing handle it.
            return Ok(serde_json::to_vec(&false).unwrap());
        }

        let req = serde_json::json!({
            "topic": "media_control",
            "action": action
        });

        let _ = query::<serde_json::Value>(&serde_json::to_string(&req).unwrap());
        return Ok(serde_json::to_vec(&true).unwrap());
    }

    Ok(serde_json::to_vec(&false).unwrap())
}
