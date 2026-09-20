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
        id: "crypto_coin".to_string(),
        name: "3D Rotating Coin".to_string(),
        author: "zius".to_string(),
        version: "1.1.0".to_string(),
        description: "High-detail mathematically rendered 3D crypto coin".to_string(),
        api_version: "0.9.0".to_string(),
    };
    Ok(serde_json::to_vec(&meta)?)
}

#[plugin_fn]
pub fn widgets() -> FnResult<Vec<u8>> {
    let widgets = vec!["coin"];
    Ok(serde_json::to_vec(&widgets)?)
}

static mut TICK: u64 = 0;

fn emblem_map(u: f32, v: f32) -> (bool, bool) {
    let r = (u * u + v * v).sqrt();
    let is_border = r >= 0.82 && r <= 0.88;

    let in_stem = u >= -0.22 && u <= -0.10 && v >= -0.55 && v <= 0.55;
    let in_prongs = ((v >= 0.55 && v <= 0.70) || (v >= -0.70 && v <= -0.55))
        && ((u >= -0.20 && u <= -0.14) || (u >= 0.04 && u <= 0.10));

    let top_d = ((u - 0.0).powi(2) + (v - (-0.25)).powi(2)).sqrt();
    let in_top_loop = top_d <= 0.28 && top_d >= 0.12 && u >= -0.15;

    let bot_d = ((u - 0.03).powi(2) + (v - 0.25).powi(2)).sqrt();
    let in_bot_loop = bot_d <= 0.32 && bot_d >= 0.14 && u >= -0.15;

    let in_bars = (v.abs() <= 0.06 && u >= -0.20 && u <= 0.15)
        || ((v - (-0.50)).abs() <= 0.06 && u >= -0.20 && u <= 0.12)
        || ((v - 0.50).abs() <= 0.06 && u >= -0.20 && u <= 0.15);

    (
        in_stem || in_prongs || in_top_loop || in_bot_loop || in_bars,
        is_border,
    )
}

fn render_3d_coin(tick: u64, width: usize, height: usize) -> Vec<serde_json::Value> {
    let mut z_buffer = vec![0.0f32; width * height];
    let mut b_buffer = vec![' '; width * height];
    let mut c_buffer = vec!["#78350f"; width * height];

    let t = (tick as f32 / 30.0) * 2.2;
    let rot_x = 0.35f32;
    let rot_y = t;
    let rot_z = 0.08f32;

    let (sinX, cosX) = rot_x.sin_cos();
    let (sinY, cosY) = rot_y.sin_cos();
    let (sinZ, cosZ) = rot_z.sin_cos();

    let rotate = |x: f32, y: f32, z: f32| -> (f32, f32, f32) {
        let y1 = y * cosX - z * sinX;
        let z1 = y * sinX + z * cosX;
        let x2 = x * cosY + z1 * sinY;
        let z2 = -x * sinY + z1 * cosY;
        let x3 = x2 * cosZ - y1 * sinZ;
        let y3 = x2 * sinZ + y1 * cosZ;
        (x3, y3, z2)
    };

    let r_coin = 1.8f32;
    let half_t = 0.22f32;
    let k2 = 5.0f32;
    let scale = (width as f32).min(height as f32 * 2.1) * k2 * 0.95 / (2.0 * r_coin);

    let lx = 0.577f32;
    let ly = -0.577f32;
    let lz = -0.577f32;
    let vz = -1.0f32;

    let ramp = " .'`^,:;Il!i><~+_-?][}{1)(|\\/tfjrxnuvczXYUJCLQ0OZmwqpdbkhao*#MW&8%B@$";
    let ramp_len = ramp.len() - 1;

    // 1. Faces
    for face_sign in [1.0f32, -1.0f32] {
        let z_face = face_sign * half_t;
        for r_step in 1..=24 {
            let r = (r_step as f32 / 24.0) * r_coin;
            let norm_r = r / r_coin;
            for a_step in 0..120 {
                let theta = (a_step as f32 / 120.0) * std::f32::consts::TAU;
                let (sinT, cosT) = theta.sin_cos();
                let ox = r * cosT;
                let oy = r * sinT;

                let (is_emblem, is_border) = if face_sign > 0.0 {
                    emblem_map(ox / r_coin, oy / r_coin)
                } else {
                    let is_ring = (norm_r * 6.0).fract() < 0.2;
                    let is_star = ((theta * 4.0).sin().abs() > 0.75) && norm_r < 0.75;
                    (is_star, is_ring)
                };

                let mut nx = 0.0f32;
                let mut ny = 0.0f32;
                let mut nz = face_sign;
                let mut bump = 0.0f32;

                if is_emblem {
                    nz *= 0.85;
                    nx += 0.3 * cosT;
                    ny += 0.3 * sinT;
                    bump = 0.35;
                } else if is_border {
                    bump = 0.25;
                }

                let n_len = (nx * nx + ny * ny + nz * nz).sqrt().max(0.001);
                let (rx, ry, rz) = rotate(ox, oy, z_face);
                let (rnx, rny, rnz) = rotate(nx / n_len, ny / n_len, nz / n_len);

                let z_depth = k2 + rz;
                if z_depth <= 0.1 {
                    continue;
                }
                let ooz = 1.0 / z_depth;

                let xp = (width as f32 / 2.0 + scale * ooz * rx) as i32;
                let yp = (height as f32 / 2.0 - (scale * 0.50) * ooz * ry) as i32;

                if xp >= 0 && xp < width as i32 && yp >= 0 && yp < height as i32 {
                    let idx = (yp * width as i32 + xp) as usize;
                    let diff = -(rnx * lx + rny * ly + rnz * lz).max(0.0);
                    let hx = lx;
                    let hy = ly;
                    let hz = lz + vz;
                    let h_len = (hx * hx + hy * hy + hz * hz).sqrt().max(0.001);
                    let spec = (-(rnx * (hx / h_len) + rny * (hy / h_len) + rnz * (hz / h_len)))
                        .max(0.0)
                        .powi(8);

                    let lum = (0.20 + 0.65 * diff + bump).clamp(0.0, 1.0);

                    if ooz > z_buffer[idx] {
                        z_buffer[idx] = ooz;
                        let char_idx = (lum * ramp_len as f32) as usize;
                        b_buffer[idx] =
                            ramp.chars().nth(char_idx.clamp(0, ramp_len)).unwrap_or('█');
                        c_buffer[idx] = if spec > 0.4 {
                            "#fffbeb" // Bright gold/white specular
                        } else if lum > 0.6 {
                            "#fbbf24" // Bright amber/gold
                        } else if lum > 0.35 {
                            "#d97706" // Warm gold midtone
                        } else {
                            "#78350f" // Bronze shadow
                        };
                    }
                }
            }
        }
    }

    // 2. Reeded Rim
    for z_s in 0..=6 {
        let z_coord = -half_t + (z_s as f32 / 6.0) * (2.0 * half_t);
        for a_s in 0..140 {
            let theta = (a_s as f32 / 140.0) * std::f32::consts::TAU;
            let (sinT, cosT) = theta.sin_cos();
            let reeding = (theta * 36.0).cos();
            let nx = cosT + 0.15 * reeding * cosT;
            let ny = sinT + 0.15 * reeding * sinT;
            let n_len = (nx * nx + ny * ny).sqrt().max(0.001);

            let (rx, ry, rz) = rotate(r_coin * cosT, r_coin * sinT, z_coord);
            let (rnx, rny, rnz) = rotate(nx / n_len, ny / n_len, 0.0);

            let z_depth = k2 + rz;
            if z_depth <= 0.1 {
                continue;
            }
            let ooz = 1.0 / z_depth;

            let xp = (width as f32 / 2.0 + scale * ooz * rx) as i32;
            let yp = (height as f32 / 2.0 - (scale * 0.50) * ooz * ry) as i32;

            if xp >= 0 && xp < width as i32 && yp >= 0 && yp < height as i32 {
                let idx = (yp * width as i32 + xp) as usize;
                let diff = -(rnx * lx + rny * ly + rnz * lz).max(0.0);
                let lum = (0.25 + 0.70 * diff + 0.12 * reeding.abs()).clamp(0.0, 1.0);

                if ooz > z_buffer[idx] {
                    z_buffer[idx] = ooz;
                    let char_idx = (lum * ramp_len as f32) as usize;
                    b_buffer[idx] = ramp.chars().nth(char_idx.clamp(0, ramp_len)).unwrap_or('#');
                    c_buffer[idx] = if lum > 0.6 {
                        "#f59e0b"
                    } else if lum > 0.35 {
                        "#b45309"
                    } else {
                        "#78350f"
                    };
                }
            }
        }
    }

    let mut lines = Vec::with_capacity(height);
    for y in 0..height {
        let mut spans = Vec::new();
        let mut current_str = String::new();
        let mut current_col = "";

        for x in 0..width {
            let idx = y * width + x;
            let ch = b_buffer[idx];
            let col = c_buffer[idx];

            if col != current_col && !current_str.is_empty() {
                spans.push(json!({
                    "content": current_str,
                    "style": { "fg": current_col, "bold": true }
                }));
                current_str = String::new();
            }

            current_col = col;
            current_str.push(ch);
        }

        if !current_str.is_empty() {
            spans.push(json!({
                "content": current_str,
                "style": { "fg": current_col, "bold": true }
            }));
        }

        lines.push(json!({ "spans": spans }));
    }

    lines
}

#[plugin_fn]
pub fn render_widget(widget_id: String) -> FnResult<Vec<u8>> {
    if widget_id == "coin" {
        let tick = unsafe {
            TICK = TICK.wrapping_add(1);
            TICK
        };

        let lines = render_3d_coin(tick, 44, 20);

        let ui = json!({
            "type": "Paragraph",
            "block": {
                "title": " 🪙 3D Crypto Coin ",
                "bordered": true,
                "border_color": "yellow"
            },
            "lines": lines,
            "wrap": false
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
