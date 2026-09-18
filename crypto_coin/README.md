# Crypto Coin Extension

A fun community extension that adds a rotating 3D ASCII coin to your Vanta dashboard.

## Installation

```bash
vanta install crypto_coin
vanta enable crypto_coin
```

## Adding to Dashboard

Open your `~/.config/vanta/config.toml` and drop the `"coin"` widget ID into your dashboard layout array. For example, to replace the `gauges` widget:

```toml
[dashboard]
layout = [
    ["system", "coin", "cpu", "storage"],
    ["clock", "media", "visualizer", "processes"],
    ["status", "weather", "memory", "network", "calendar"]
]
```
