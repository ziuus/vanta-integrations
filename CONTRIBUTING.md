# Contributing to Vanta Integrations

Welcome! This repository serves as the official registry for community-built Vanta extensions.

Vanta uses a **Decentralized Registry Model** (similar to Raycast or Neovim plugins). You do **not** need to submit your source code to this repository. You only need to add your extension to the `registry.json` file.

## How to Create and Publish a Vanta Extension

### Step 1: Build Your Extension
Use our provided template to scaffold your WASM micro-extension.

```bash
# Clone this repository just to get the template
git clone https://github.com/ziuus/vanta-integrations
cp -r vanta-integrations/vanta-extension-template my-vanta-extension
cd my-vanta-extension
```
Edit `src/lib.rs` and `Cargo.toml` to build your desired functionality. Use `vanta link /path/to/wasm` to test it locally in Vanta.

### Step 2: Host on GitHub and Release
1. Create a new public repository for your extension on your own GitHub account.
2. Push your code.
3. Build your plugin for release (`cargo build --target wasm32-unknown-unknown --release`).
4. Create a **GitHub Release** in your repository and attach the compiled `.wasm` file as an asset.

### Step 3: Add to the Registry
To make your extension available to all Vanta users via `vanta search` and `vanta install`:

1. Fork this `vanta-integrations` repository.
2. Edit `registry.json` and append your extension's metadata. 
   - `id`: The unique ID for your extension
   - `name`: Human-readable name
   - `description`: Short description
   - `author`: Your name/handle
   - `repo`: URL to your GitHub repository
   - `wasmUrl`: Direct download URL to the `.wasm` file in your GitHub Release
   - `sha256`: The SHA-256 hash of your `.wasm` file (run `sha256sum your_plugin.wasm`)
3. Open a Pull Request!

Once your PR is merged, your extension will immediately appear in the `vanta search` CLI and on the Vanta integrations website.
