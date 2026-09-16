# Vanta Community Integrations 🧩

This repository contains official and community-contributed extensions, widgets, and pages for [Vanta](https://github.com/ziuus/vanta).

---

## 🔒 Security & Trust Model

**Important Security Notice**: Extensions in Vanta are trusted Rust code compiled directly into the Vanta executable. **They are NOT sandboxed.** 

Always inspect third-party extension source code before adding it to your Vanta build.

---

## 📦 Available Integrations

| Extension | Crate Name | Description |
| :--- | :--- | :--- |
| **Security Pack** | `vanta-security` | Live CVE security feeds and threat monitoring components. |

---

## 🛠️ How to Use an Integration in Your Vanta Build

Vanta v0.8.0 introduces the **V1 Extension API**. Because Vanta binaries are compiled for maximum performance, incorporating an extension into your local Vanta binary involves 3 quick steps:

### Step 1: Add the Dependency
In your local Vanta repository's `Cargo.toml`, add the target integration crate:

```toml
[dependencies]
vanta-security = { git = "https://github.com/ziuus/vanta-integrations", package = "vanta-security" }
```

### Step 2: Register in `src/main.rs`
Open `src/main.rs` in Vanta and register the extension:

```rust
app.ext_manager.register(
    Box::new(vanta_security::SecurityExtension),
    app.config.extensions.as_ref()
);
```

### Step 3: Enable & Place in `config.toml`
Recompile Vanta (`cargo build --release`). Then enable the integration in `~/.config/vanta/config.toml`:

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

---

## 🚀 How to Build & Contribute an Integration

Want to build and share your own extension for Vanta?

### 1. Create a Library Crate
Fork this repository or create a new library crate:
```bash
cargo new --lib my-extension
```

### 2. Implement the V1 Extension API
Add `vanta` as a dependency in your crate's `Cargo.toml`:
```toml
[dependencies]
vanta = { git = "https://github.com/ziuus/vanta" }
ratatui = "0.29"
```

In `src/lib.rs`, implement `vanta::extension::Extension` and `vanta::extension::Component` (or `vanta::extension::Page`):

```rust
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use vanta::extension::{Component, Extension, ExtensionMetadata};
use vanta::theme::Theme;

pub struct MyWidget;

impl Component for MyWidget {
    fn id(&self) -> &'static str {
        "my_widget"
    }

    fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        let block = Block::default().borders(Borders::ALL).title(" My Widget ");
        f.render_widget(Paragraph::new("Hello from extension!").block(block), area);
    }
}

pub struct MyExtension;

impl Extension for MyExtension {
    fn metadata(&self) -> ExtensionMetadata {
        ExtensionMetadata {
            id: "my_ext",
            name: "My Extension",
            author: "Your Name",
            version: "0.1.0",
            description: "A custom Vanta extension.",
        }
    }

    fn components(&self) -> Vec<Box<dyn Component>> {
        vec![Box::new(MyWidget)]
    }
}
```

### 3. Open a Pull Request
Add your crate to the workspace `Cargo.toml` in this repo and submit a PR!

---

## 📄 License
MIT License. See individual crate directories for specific licensing details if applicable.
