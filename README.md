# Vanta Community Integrations 🧩

This repository contains community-contributed extensions, widgets, and pages for [Vanta](https://github.com/ziuus/vanta).

---

## 🔒 Security & Trust Model

**Vanta v0.9+ runs extensions as secure WebAssembly (WASM) modules.**

Unlike the old compiled architecture, v0.9 extensions are **sandboxed by default**. They cannot access your filesystem, network, or spawn processes unless you explicitly grant them permission. If an extension crashes or hangs, Vanta terminates it without dropping your terminal dashboard.

---

## 📦 Available Integrations

| Extension | Module Name | Description |
| :--- | :--- | :--- |
| **Security Pack** | `vanta-security.wasm` | Live CVE security feeds and threat monitoring components running safely in WASM. |

---

## 🛠️ How to Install an Extension

With Vanta v0.9, you no longer need to fork or recompile Vanta to use extensions.

### Step 1: Download the Module
Download the `.wasm` file (e.g., `vanta-security.wasm`) and place it in your extensions folder:

```bash
mkdir -p ~/.config/vanta/extensions/
cp vanta-security.wasm ~/.config/vanta/extensions/
```

### Step 2: Enable & Place in `config.toml`
Open your `~/.config/vanta/config.toml` and enable the extension:

```toml
[extensions]
enabled = ["security"]

# Place extension components directly in your layout grid
[dashboard]
layout = [
    ["system", "cpu", "memory"],
    ["cve_feed", "processes"]
]
```

That's it! Restart Vanta, and the extension will load dynamically at runtime.

---

## 🚀 How to Build a WASM Extension

Want to build and share your own extension for Vanta? It's incredibly easy using Rust and Extism.

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
```

### 2. Implement the Vanta UI Protocol
In `src/lib.rs`, simply use the `extism_pdk` to return a JSON layout that matches Vanta's declarative UI protocol:

```rust
use extism_pdk::*;
use serde_json::json;

#[plugin_fn]
pub fn metadata() -> FnResult<Vec<u8>> {
    let meta = json!({
        "id": "my_ext",
        "name": "My Extension",
        "author": "Your Name",
        "version": "0.1.0",
        "description": "A custom Vanta extension.",
        "api_version": "0.9.0"
    });
    Ok(serde_json::to_vec(&meta)?)
}

#[plugin_fn]
pub fn widgets() -> FnResult<Vec<u8>> {
    Ok(serde_json::to_vec(&vec!["my_widget"])?)
}

#[plugin_fn]
pub fn render_widget(widget_id: String) -> FnResult<Vec<u8>> {
    if widget_id == "my_widget" {
        let ui = json!({
            "type": "Paragraph",
            "lines": [ { "spans": [ { "content": "Hello from WASM!" } ] } ],
            "block": { "title": " My Widget ", "bordered": true }
        });
        Ok(serde_json::to_vec(&ui)?)
    } else {
        Ok(vec![])
    }
}
```

### 3. Compile to WebAssembly
```bash
cargo build --release --target wasm32-unknown-unknown
```
Your compiled extension is ready at `target/wasm32-unknown-unknown/release/my_extension.wasm`.

---

## 📄 License
MIT License. See individual crate directories for specific licensing details if applicable.
