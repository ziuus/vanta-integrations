use extism_pdk::*;
use vanta_ext_sdk::{ui, Widget};
use vanta_ext_sdk::ui::{Color, Style, Line, Span, Block};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::AnimationDecoder;
use std::cell::RefCell;

#[host_fn]
extern "ExtismHost" {
    fn vanta_query(input: String) -> String;
}

thread_local! {
    static FRAMES: RefCell<Vec<image::RgbaImage>> = RefCell::new(Vec::new());
    static GIF_BYTES: RefCell<Option<Vec<u8>>> = RefCell::new(None);
    static LAST_TICK: RefCell<f64> = RefCell::new(0.0);
    static ERROR_MSG: RefCell<String> = RefCell::new(String::new());
}

const DOT_BITS: [[u8; 2]; 4] = [[0, 3], [1, 4], [2, 5], [6, 7]];

fn render_image(img: &image::RgbaImage, w: u16, h: u16) -> Vec<Line> {
    if w == 0 || h == 0 { return Vec::new(); }
    let (iw, ih) = (img.width(), img.height());
    if iw == 0 || ih == 0 { return Vec::new(); }
    
    let aspect = iw as f32 / ih as f32;
    let mut cw = w;
    let mut ch = (cw as f32 / aspect / 2.0).ceil() as u16;
    if ch > h {
        ch = h;
        cw = (ch as f32 * aspect * 2.0).ceil() as u16;
        cw = cw.min(w);
    }
    let (cw, ch) = (cw.max(1), ch.max(1));

    let px_w = cw as u32 * 2;
    let px_h = ch as u32 * 4;
    
    let small = image::imageops::resize(img, px_w, px_h, image::imageops::FilterType::Nearest);
    
    let mut lines = Vec::with_capacity(ch as usize);
    for cy in 0..ch as u32 {
        let mut spans: Vec<Span> = Vec::with_capacity(cw as usize);
        for cx in 0..cw as u32 {
            let mut luma = [0f32; 8];
            let (mut r, mut g, mut b, mut opaque) = (0u32, 0u32, 0u32, 0u32);
            for (dy, row) in DOT_BITS.iter().enumerate() {
                for (dx, _) in row.iter().enumerate() {
                    let p = small.get_pixel(cx * 2 + dx as u32, cy * 4 + dy as u32);
                    let l = p[0] as f32 * 0.299 + p[1] as f32 * 0.587 + p[2] as f32 * 0.114;
                    luma[dy * 2 + dx] = if p[3] < 50 { -1.0 } else { l };
                    if p[3] >= 50 {
                        r += p[0] as u32; g += p[1] as u32; b += p[2] as u32; opaque += 1;
                    }
                }
            }
            if opaque == 0 {
                spans.push(ui::raw(" "));
                continue;
            }
            let block_mean: f32 = luma.iter().filter(|l| **l >= 0.0).sum::<f32>() / opaque as f32;
            let mut pattern = 0u8;
            for (dy, row) in DOT_BITS.iter().enumerate() {
                for (dx, bit) in row.iter().enumerate() {
                    let l = luma[dy * 2 + dx];
                    if l >= 0.0 && l >= 128.0 {
                        pattern |= 1 << bit;
                    }
                }
            }
            if pattern == 0 && block_mean >= 128.0 { pattern = 0xFF; }
            if pattern == 0 {
                spans.push(ui::raw(" "));
                continue;
            }
            let ch_out = char::from_u32(0x2800 + pattern as u32).unwrap_or(' ');
            let color = Color::Rgb((r / opaque) as u8, (g / opaque) as u8, (b / opaque) as u8);
            spans.push(ui::span(ch_out.to_string(), Style::fg(color)));
        }
        lines.push(Line::new(spans));
    }
    lines
}

fn build_video_widget(w: u16, h: u16) -> Widget {
    let mut lines = Vec::new();
    
    GIF_BYTES.with(|gb| {
        let mut gbytes = gb.borrow_mut();
        if gbytes.is_none() {
            let req = serde_json::json!({
                "topic": "fs_read",
                "path": "/tmp/test.gif"
            });
            match unsafe { vanta_query(req.to_string()) } {
                Ok(res) => {
                    match serde_json::from_str::<serde_json::Value>(&res) {
                        Ok(val) => {
                            if let Some(b64) = val.get("data").and_then(|d| d.get("bytes_b64")).and_then(|b| b.as_str()) {
                                match STANDARD.decode(b64) {
                                    Ok(bytes) => {
                                        *gbytes = Some(bytes);
                                    }
                                    Err(e) => { ERROR_MSG.with(|m| *m.borrow_mut() = format!("b64 err: {}", e)); }
                                }
                            } else {
                                ERROR_MSG.with(|m| *m.borrow_mut() = format!("No bytes_b64"));
                            }
                        }
                        Err(e) => { ERROR_MSG.with(|m| *m.borrow_mut() = format!("json err: {}", e)); }
                    }
                }
                Err(_e) => { ERROR_MSG.with(|m| *m.borrow_mut() = format!("query err")); }
            }
        }
    });

    FRAMES.with(|f| {
        let mut frames = f.borrow_mut();
        
        GIF_BYTES.with(|gb| {
            if let Some(bytes) = gb.borrow().as_ref() {
                if frames.is_empty() {
                    match image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes)) {
                        Ok(decoder) => {
                            // Extract up to 20 frames to avoid 250ms WASM timeout
                            for frame_res in decoder.into_frames().take(3) {
                                match frame_res {
                                    Ok(frame) => frames.push(frame.into_buffer()),
                                    Err(_) => break,
                                }
                            }
                        }
                        Err(e) => { ERROR_MSG.with(|m| *m.borrow_mut() = format!("gif err: {}", e)); }
                    }
                }
            }
        });

        ERROR_MSG.with(|m| {
            let msg = m.borrow();
            if !msg.is_empty() {
                lines.push(Line::text(msg.clone(), Style::fg(Color::RED)));
            }
        });

        if frames.is_empty() {
            if lines.is_empty() {
                lines.push(Line::text("Waiting for /tmp/test.gif...", Style::dim()));
            }
        } else {
            LAST_TICK.with(|t| {
                let mut tick = t.borrow_mut();
                *tick += 0.5;
                let frame_idx = (*tick as usize) % frames.len();
                let img = &frames[frame_idx];
                let mut img_lines = render_image(img, w, h);
                lines.append(&mut img_lines);
            });
        }
    });

    Widget::paragraph(lines).block(Block::titled(" VIDEO PLAYER ".to_string()))
}

#[plugin_fn]
pub fn metadata() -> FnResult<Vec<u8>> {
    Ok(vanta_ext_sdk::ExtensionMetadata::new(
        "video_player",
        "Video Player",
        "0.1.0",
        "Plays /tmp/test.gif",
        vanta_ext_sdk::API_VERSION_TELEMETRY,
    )
    .to_json())
}

#[plugin_fn]
pub fn widgets(_: ()) -> FnResult<Vec<u8>> {
    let ids = serde_json::json!(["video_widget"]);
    Ok(serde_json::to_vec(&ids).unwrap_or_default())
}

#[plugin_fn]
pub fn render_widget(id: String) -> FnResult<Vec<u8>> {
    let widget = match id.as_str() {
        "video_widget" => build_video_widget(80, 24),
        _ => return Ok(vanta_ext_sdk::ui::unavailable("UNKNOWN", "invalid widget").to_json()),
    };
    Ok(widget.to_json())
}
