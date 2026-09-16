# Vanta Community Integrations & Extensions 🧩

This repository contains official and community-contributed extensions, widgets, and pages for [Vanta](https://github.com/ziuus/vanta).

---

## 📦 Available Integrations

| Extension | Crate Name | Description |
| :--- | :--- | :--- |
| **Security Pack** | `vanta-security` | Live CVE feeds, threat monitoring, and ClamAV components. |

---

## 🛠️ How to Use an Integration in Your Vanta Build

Because Vanta binaries are pre-compiled for performance, integrating a community crate into your Vanta executable takes just 3 quick steps:

### Step 1: Add the Dependency
In your local Vanta repository's `Cargo.toml`, add the integration crate from this repo:

```toml
[dependencies]
vanta-security = { git = "https://github.com/ziuus/vanta-integrations", package = "vanta-security" }
```

### Step 2: Register in `src/main.rs`
Open `src/main.rs` in Vanta and register the extension instance:

```rust
app.ext_manager.register(
    Box::new(vanta_security::SecurityExtension),
    app.config.extensions.as_ref()
);
```

### Step 3: Enable in `config.toml`
Build Vanta (`cargo build --release`). Then enable the integration in `~/.config/vanta/config.toml`:

```toml
[extensions]
enabled = ["security"]

# (Optional) Place security components directly on your main dashboard
[dashboard]
layout = [
    ["system", "cpu", "memory"],
    ["cve_feed", "processes"]
]
```

---

## 🚀 Contributing a New Integration

Want to build and share your own extension for Vanta?

1. Fork this repository.
2. Create a new library crate using `cargo new --lib your-extension-name`.
3. Add `vanta` as a dependency in your extension's `Cargo.toml`:
   ```toml
   [dependencies]
   vanta = { git = "https://github.com/ziuus/vanta" }
   ratatui = "0.29"
   ```
4. Implement the `Extension` trait from `vanta::extension`.
5. Add your crate to the workspace `Cargo.toml` in the root of this repo.
6. Open a Pull Request!
