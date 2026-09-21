# MediaDeck

A premium, interactive terminal media center for Vanta.

## Configuration

Add the following to your Vanta `config.toml`:

```toml
[extensions]
enabled = ["mediadeck"]

[[pages]]
name = "MediaDeck"
layout = [
  ["media_now_playing", "media_visualizer"],
  ["media_transport", "media_signal"],
  ["media_queue", "media_players"]
]
```
