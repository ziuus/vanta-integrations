import os
import shutil

crates_def = {
    "iowatch": {
        "main": ("iowatch", "build_iowatch(80, 24)"),
        "top": ("io_top", "build_io_top(80, 24)"),
        "activity": ("io_activity", "build_io_activity(80, 24)")
    },
    "netscope": {
        "main": ("netscope", "build_netscope(80, 24, true, true, true)"),
        "table": ("netscope_table", "build_netscope(80, 24, true, false, false)"),
        "activity": ("netscope_activity", "build_netscope(80, 24, false, false, true)"),
        "summary": ("netscope_summary", "build_netscope(80, 24, false, true, false)")
    },
    "portwatch": {
        "main": ("portwatch", "build_portwatch(80, 24)"),
        "listeners": ("port_listeners", "build_port_listeners(80, 24)"),
        "activity": ("port_activity", "build_port_activity(80, 24)")
    },
    "proctrace": {
        "main": ("proctrace", "build_proctrace(80, 24)"),
        "activity": ("proctrace_activity", "build_activity(80, 24)")
    },
    "servicewatch": {
        "main": ("servicewatch", "build_servicewatch(80, 24)"),
        "activity": ("servicewatch_activity", "build_activity(80, 24)")
    }
}

for parent, widgets in crates_def.items():
    # read original
    with open(f"{parent}/src/lib.rs", "r") as f:
        lines = f.readlines()
        
    # find where #[plugin_fn] starts
    idx = -1
    for i, line in enumerate(lines):
        if line.strip() == "#[plugin_fn]":
            idx = i
            break
            
    base_lines = lines[:idx] if idx != -1 else lines

    for w, (widget_id, func) in widgets.items():
        new_crate = f"{parent}_{w}"
        
        os.makedirs(f"{new_crate}/src", exist_ok=True)
        with open(f"{new_crate}/Cargo.toml", "w") as f:
            f.write(f'''[package]
name = "{new_crate}"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
extism-pdk = "1.4.1"
serde = {{ version = "1.0", features = ["derive"] }}
serde_json = "1.0"
vanta-ext-sdk = {{ path = "../sdk" }}
''')

        tail = f'''
#[plugin_fn]
pub fn metadata() -> FnResult<Vec<u8>> {{
    Ok(vanta_ext_sdk::ExtensionMetadata::new(
        "{new_crate}",
        "{parent.title()} {w.title()}",
        "0.1.0",
        "Micro-extension",
        vanta_ext_sdk::API_VERSION_TELEMETRY,
    )
    .to_json())
}}

#[plugin_fn]
pub fn widgets(_: ()) -> FnResult<Vec<u8>> {{
    let ids = serde_json::json!(["{widget_id}"]);
    Ok(serde_json::to_vec(&ids).unwrap_or_default())
}}

#[plugin_fn]
pub fn render_widget(id: String) -> FnResult<Vec<u8>> {{
    if id != "{widget_id}" {{
        return Ok(vanta_ext_sdk::ui::unavailable("UNKNOWN", "invalid widget").to_json());
    }}
    let widget = {func};
    Ok(widget.to_json())
}}
'''
        with open(f"{new_crate}/src/lib.rs", "w") as f:
            if "vanta_ext_sdk::API_VERSION_TELEMETRY" not in "".join(base_lines):
                f.write("use vanta_ext_sdk::API_VERSION_TELEMETRY;\n")
            f.write("".join(base_lines) + tail)

    # We will remove the parent later
