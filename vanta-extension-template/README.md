# Vanta Extension Template

A starter template for building custom WASM micro-extensions for [Vanta](https://github.com/ziuus/vanta).

## Prerequisites
- Rust and Cargo installed
- `wasm32-unknown-unknown` target installed:
  ```bash
  rustup target add wasm32-unknown-unknown
  ```

## Building
Compile your extension to WebAssembly:
```bash
cargo build --target wasm32-unknown-unknown --release
```
The compiled plugin will be at `target/wasm32-unknown-unknown/release/my_vanta_extension.wasm`.

## Testing Locally
You can test your extension instantly without publishing it:
```bash
# Link the compiled WASM to Vanta
vanta link ./target/wasm32-unknown-unknown/release/my_vanta_extension.wasm

# Start Vanta and configure your layout to use 'my_widget'
vanta
```

## Publishing
To share your extension with the community:
1. Push your repository to GitHub.
2. Publish a GitHub Release containing your `.wasm` file.
3. Open a Pull Request on the [vanta-integrations](https://github.com/ziuus/vanta-integrations) repository to add your extension to the community registry.
