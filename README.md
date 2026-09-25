# Vanta Community Integrations 🧩

This repository contains community-contributed extensions, widgets, and pages for [Vanta](https://github.com/ziuus/vanta).

---

## 📂 Repository Structure

- `components/` - Standalone widgets and UI components (e.g. `dashboard_github`, `filespace_browser`).
- `pages/` - Full-page WASM extensions or complete TOML dashboard layout templates.
- `scenes/` - Full-screen background ambient renderers (e.g. `scene_starfield`).
- `themes/` - Community-contributed theme definitions (TOML color palettes).
- `sdk/` - The `vanta-ext-sdk` used to build WASM plugins.
- `scripts/` - Maintenance and build scripts.
- `docs/` - Technical documentation.

## 🔒 Security & Trust Model

**Vanta runs extensions as secure WebAssembly (WASM) modules.**

Extensions are **sandboxed by default**. They cannot access your filesystem, network, or spawn processes. Instead, they interact with the host through highly constrained, safe JSON queries (e.g., `fs_list`, `media`, `state_get`).

## 🧱 The V2 Micro-Extension Architecture

To maximize customization, Vanta integrations use a **Micro-Extension Pattern**. 
Instead of monolithic applications, **every single widget is its own independent `.wasm` file**. 

If you want a File Manager, you don't install one massive plugin. You install the `filespace_browser`, `filespace_preview`, and `filespace_queue` micro-extensions, and lay them out on a page. Under the hood, they communicate via Vanta's secure **Host State Mailbox** (`state_get` / `state_set`).

---

## 📦 Available Integrations

### Tier 1: Deep Observability
| Extension | Description |
| :--- | :--- |
| **`sentinel`** | Anomaly detection & persistence tracking graph. |
| **`iowatch`** | Process-level disk I/O attribution and history. |
| **`portwatch`** | Open ports and their owning processes. |
| **`netscope`** | Live socket tracking (who is talking to whom). |
| **`proctrace`** | Hierarchical process ancestry trees. |
| **`servicewatch`** | Systemd service lifecycles and failures. |

### Tier 2: Micro-Extension Apps
| Application | Micro-Extensions (WASM crates) | Description |
| :--- | :--- | :--- |
| **CryptoPulse** | `cryptopulse`, `crypto_coin` | Live cryptocurrency market terminals and 3D coin rendering. |
| **MediaDeck** | `mediadeck` *(pending V2 split)* | Full MPRIS/DBus audio workstation (visualizers, players). |
| **FileSpace** | `filespace_browser`, `filespace_preview`, `filespace_sidebar`, `filespace_queue`, `filespace_path` | A complete, native-feeling terminal file manager with safe background operations. |

---

## 🛠️ How to Install

### Step 1: Download the Modules
Download the `.wasm` files (e.g., `filespace_browser.wasm`, `filespace_preview.wasm`) and place them in your extensions folder:

```bash
mkdir -p ~/.config/vanta/extensions/
cp *.wasm ~/.config/vanta/extensions/
```

### Step 2: Enable & Place in `config.toml`
Open your `~/.config/vanta/config.toml` and enable the extensions you want:

```toml
[extensions]
enabled = [
  "filespace_browser",
  "filespace_preview",
  "filespace_sidebar"
]

# Build your custom workspace using the widget IDs
[[pages]]
name = "File Space"
layout = [
    ["filespace_sidebar", "filespace_browser", "filespace_preview"]
]
```

That's it! Restart Vanta, and the extensions will load dynamically at runtime.

---

## 🚀 How to Build a WASM Micro-Extension

We use `extism-pdk` and `vanta-ext-sdk`.

### 1. Create a cdylib Crate
```bash
cargo new --lib my-extension
```

Add this to your `Cargo.toml`:
```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
extism-pdk = "1.4"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
vanta-ext-sdk = { path = "../sdk" }
```

### 2. Implement the Vanta UI Protocol
In `src/lib.rs`:

```rust
use extism_pdk::*;
use vanta_ext_sdk::{
    telemetry::query,
    ui::{Widget, Block, Line, Span, Style, Color, unavailable},
};

#[plugin_fn]
pub fn widgets(_: ()) -> FnResult<Vec<u8>> {
    Ok(serde_json::to_vec(&vec!["my_widget"])?)
}

#[plugin_fn]
pub fn render_widget(id: String) -> FnResult<Vec<u8>> {
    if id == "my_widget" {
        let widget = Widget::paragraph(vec![]).block(Block::titled(" MY WIDGET "));
        Ok(widget.to_json())
    } else {
        Ok(unavailable("UNKNOWN", "Invalid widget id").to_json())
    }
}
```

### 3. State Mailbox (Inter-Extension Communication)
Because each widget is a separate sandboxed `.wasm` file, they share data via the host mailbox:

**Writer (e.g., a Browser list):**
```rust
let req = serde_json::json!({ "topic": "state_set", "key": "selected_file", "value": "/etc/hosts" });
let _ = query::<serde_json::Value>(&req.to_string());
```

**Reader (e.g., a Preview pane):**
```rust
let req = serde_json::json!({ "topic": "state_get", "key": "selected_file" });
let val = query::<serde_json::Value>(&req.to_string())?;
// Parse the value and render...
```
