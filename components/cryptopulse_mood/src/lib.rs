use extism_pdk::*;
use serde::{Deserialize, Serialize};
use vanta_ext_sdk::{
    telemetry::{query, TelemetryError},
    ui, Block, Color, Line, Span, Style, Widget, API_VERSION_TELEMETRY,
};

#[derive(Clone, Default, Serialize, Deserialize, Debug)]
pub struct CryptoAsset {
    pub symbol: String,
    pub price: f64,
    pub change_24h_pct: f64,
    pub volume_24h: f64,
    pub high_24h: f64,
    pub low_24h: f64,
}

#[derive(Clone, Default, Serialize, Deserialize, Debug)]
pub struct CryptoSnapshot {
    pub assets: Vec<CryptoAsset>,
    pub fear_and_greed: Option<u32>,
    pub ready: bool,
}




fn fetch_data() -> Result<CryptoSnapshot, TelemetryError> {
    let val = query("crypto")?;
    serde_json::from_value(val).map_err(|_| TelemetryError::Decode("bad crypto json".into()))
}

fn format_price(p: f64) -> String {
    if p < 1.0 {
        format!("{:.4}", p)
    } else {
        format!("{:.2}", p)
    }
}

fn format_vol(v: f64) -> String {
    if v >= 1_000_000_000.0 {
        format!("{:.2}B", v / 1_000_000_000.0)
    } else if v >= 1_000_000.0 {
        format!("{:.2}M", v / 1_000_000.0)
    } else {
        format!("{:.0}", v)
    }
}

fn build_overview(_width: u16, _height: u16) -> Widget {
    let Ok(snap) = fetch_data() else {
        return ui::unavailable("CRYPTO OVERVIEW", "no data");
    };
    if snap.assets.is_empty() {
        return ui::unavailable("CRYPTO OVERVIEW", "no data");
    }

    let btc = snap
        .assets
        .iter()
        .find(|a| a.symbol == "BTC")
        .cloned()
        .unwrap_or_default();
    let eth = snap
        .assets
        .iter()
        .find(|a| a.symbol == "ETH")
        .cloned()
        .unwrap_or_default();

    let mut lines = vec![];

    lines.push(Line::new(vec![
        Span {
            content: format!("{:<6}", "ASSET"),
            style: Some(Style::dim()),
        },
        Span {
            content: format!("{:>12}", "PRICE"),
            style: Some(Style::dim()),
        },
        Span {
            content: format!("{:>10}", "24H CHG"),
            style: Some(Style::dim()),
        },
        Span {
            content: format!("{:>12}", "24H VOL"),
            style: Some(Style::dim()),
        },
    ]));

    for a in [&btc, &eth] {
        if a.symbol.is_empty() {
            continue;
        }
        let c = if a.change_24h_pct >= 0.0 {
            Color::GREEN
        } else {
            Color::RED
        };
        let sign = if a.change_24h_pct >= 0.0 { "+" } else { "" };
        lines.push(Line::new(vec![
            Span {
                content: format!("{:<6}", a.symbol),
                style: Some(Style::fg(Color::WHITE).bold()),
            },
            Span {
                content: format!("{:>12}", format!("${}", format_price(a.price))),
                style: Some(Style::fg(Color::CYAN)),
            },
            Span {
                content: format!("{:>10}", format!("{}{:.2}%", sign, a.change_24h_pct)),
                style: Some(Style::fg(c)),
            },
            Span {
                content: format!("{:>12}", format_vol(a.volume_24h)),
                style: Some(Style::fg(Color::DARK_GRAY)),
            },
        ]));
    }

    Widget::paragraph(lines).block(Block::titled(" MARKET OVERVIEW ".to_string()))
}

fn build_movers(_width: u16, height: u16) -> Widget {
    let Ok(snap) = fetch_data() else {
        return ui::unavailable("TOP MOVERS", "no data");
    };
    if snap.assets.is_empty() {
        return ui::unavailable("TOP MOVERS", "no data");
    }

    let mut sorted = snap.assets.clone();
    sorted.sort_by(|a, b| {
        b.change_24h_pct
            .partial_cmp(&a.change_24h_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let max_items = (height.saturating_sub(2) / 2).max(1) as usize;

    let gainers: Vec<_> = sorted.iter().take(max_items).collect();
    let losers: Vec<_> = sorted.iter().rev().take(max_items).collect();

    let mut lines = vec![];
    lines.push(Line::new(vec![Span {
        content: "TOP GAINERS".into(),
        style: Some(Style::fg(Color::GREEN).bold()),
    }]));
    for g in gainers {
        lines.push(Line::new(vec![
            Span {
                content: format!("{:<6}", g.symbol),
                style: Some(Style::fg(Color::WHITE)),
            },
            Span {
                content: format!("{:>8}", format!("+{}%", g.change_24h_pct)),
                style: Some(Style::fg(Color::GREEN)),
            },
        ]));
    }

    lines.push(Line::new(vec![]));
    lines.push(Line::new(vec![Span {
        content: "TOP LOSERS".into(),
        style: Some(Style::fg(Color::RED).bold()),
    }]));
    for l in losers {
        lines.push(Line::new(vec![
            Span {
                content: format!("{:<6}", l.symbol),
                style: Some(Style::fg(Color::WHITE)),
            },
            Span {
                content: format!("{:>8}", format!("{}%", l.change_24h_pct)),
                style: Some(Style::fg(Color::RED)),
            },
        ]));
    }

    Widget::paragraph(lines).block(Block::titled(" TOP MOVERS ".to_string()))
}

fn build_mood(_width: u16, _height: u16) -> Widget {
    let Ok(snap) = fetch_data() else {
        return ui::unavailable("MARKET MOOD", "no data");
    };

    let fng = snap.fear_and_greed.unwrap_or(50);

    let (label, color) = match fng {
        0..=25 => ("Extreme Fear", Color::RED),
        26..=45 => ("Fear", Color::LIGHT_RED),
        46..=54 => ("Neutral", Color::DARK_GRAY),
        55..=75 => ("Greed", Color::LIGHT_GREEN),
        76..=100 => ("Extreme Greed", Color::GREEN),
        _ => ("Unknown", Color::GRAY),
    };

    let ratio = (fng as f64) / 100.0;

    Widget::Gauge {
        ratio,
        label: Some(format!(" {} - {} ", fng, label)),
        block: Some(Block::titled(" FEAR & GREED ".to_string())),
        color: Some(color),
    }
}

fn build_watchlist(_width: u16, height: u16) -> Widget {
    let Ok(snap) = fetch_data() else {
        return ui::unavailable("WATCHLIST", "no data");
    };

    let wl = ["SOL", "XRP", "ADA", "DOGE", "DOT"];
    let mut lines = vec![];

    let max = height.saturating_sub(2) as usize;
    for (_i, sym) in wl.iter().enumerate().take(max) {
        if let Some(a) = snap.assets.iter().find(|x| x.symbol == *sym) {
            let c = if a.change_24h_pct >= 0.0 {
                Color::GREEN
            } else {
                Color::RED
            };
            let sign = if a.change_24h_pct >= 0.0 { "+" } else { "" };
            lines.push(Line::new(vec![
                Span {
                    content: format!("{:<6}", a.symbol),
                    style: Some(Style::fg(Color::WHITE)),
                },
                Span {
                    content: format!("{:>8}", format!("${:.2}", a.price)),
                    style: Some(Style::fg(Color::CYAN)),
                },
                Span {
                    content: format!("{:>10}", format!("{}{:.2}%", sign, a.change_24h_pct)),
                    style: Some(Style::fg(c)),
                },
            ]));
        }
    }

    Widget::paragraph(lines).block(Block::titled(" WATCHLIST ".to_string()))
}



fn build_stats(_width: u16, _height: u16) -> Widget {
    let Ok(snap) = fetch_data() else {
        return ui::unavailable("MARKET STATS", "no data");
    };
    if snap.assets.is_empty() {
        return ui::unavailable("MARKET STATS", "no data");
    }

    let total_vol: f64 = snap.assets.iter().map(|a| a.volume_24h).sum();
    let btc_price = snap
        .assets
        .iter()
        .find(|a| a.symbol == "BTC")
        .map(|a| a.price)
        .unwrap_or(0.0);

    let mut lines = vec![];
    lines.push(Line::new(vec![
        Span {
            content: "24H VOL: ".into(),
            style: Some(Style::dim()),
        },
        Span {
            content: format!("${}", format_vol(total_vol)),
            style: Some(Style::fg(Color::WHITE).bold()),
        },
        Span {
            content: "   BTC: ".into(),
            style: Some(Style::dim()),
        },
        Span {
            content: format!("${:.0}", btc_price),
            style: Some(Style::fg(Color::CYAN).bold()),
        },
    ]));

    Widget::paragraph(lines).block(Block::titled(" GLOBAL STATS ".to_string()))
}

fn build_heatmap(_width: u16, height: u16) -> Widget {
    let Ok(snap) = fetch_data() else {
        return ui::unavailable("MARKET HEATMAP", "no data");
    };

    let mut lines = vec![];
    let max = height.saturating_sub(2) as usize;

    // Simplistic chunking for a "heatmap" grid effect
    for chunk in snap.assets.chunks(4).take(max) {
        let mut row = vec![];
        for a in chunk {
            let color = if a.change_24h_pct >= 2.0 {
                Color::GREEN
            } else if a.change_24h_pct > 0.0 {
                Color::LIGHT_GREEN
            } else if a.change_24h_pct <= -2.0 {
                Color::RED
            } else {
                Color::LIGHT_RED
            };

            row.push(Span {
                content: format!("{:>5} ", a.symbol),
                style: Some(Style::fg(color).bold()),
            });
        }
        lines.push(Line::new(row));
    }

    Widget::paragraph(lines).block(Block::titled(" HEATMAP ".to_string()))
}




#[plugin_fn]
pub fn metadata() -> FnResult<Vec<u8>> {
    Ok(vanta_ext_sdk::ExtensionMetadata::new(
        "cryptopulse_mood",
        "CryptoPulse Mood",
        "0.1.0",
        "Mood component for CryptoPulse.",
        API_VERSION_TELEMETRY,
    )
    .to_json())
}

#[plugin_fn]
pub fn widgets(_: ()) -> FnResult<Vec<u8>> {
    let ids = serde_json::json!(["crypto_mood"]);
    Ok(serde_json::to_vec(&ids).unwrap_or_default())
}

#[plugin_fn]
pub fn render_widget(id: String) -> FnResult<Vec<u8>> {
    if id != "crypto_mood" {
        return Ok(vanta_ext_sdk::ui::unavailable("UNKNOWN", "invalid widget").to_json());
    }
    let widget = build_mood(80, 20);
    Ok(widget.to_json())
}
