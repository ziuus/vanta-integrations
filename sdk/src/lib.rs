//! Shared SDK for Vanta WASM extensions.
//!
//! Three layers, deliberately separable so data acquisition can be tested and
//! reasoned about independently of presentation:
//!
//! * [`telemetry`] — typed client for the host's `vanta_query` function.
//! * [`history`] — bounded, wall-clock-throttled rolling buffers.
//! * [`ui`] — protocol widgets plus the primitives (sparkline, bar, table)
//!   that the host protocol does not provide.
//! * [`viz`] — denser visualizations: state machines, threshold gauges,
//!   duration meters, activity strips, signal strips with markers.
//!
//! See `docs/integration-development.md` for the platform constraints that
//! shape all three.

pub mod history;
pub mod telemetry;
pub mod ui;
pub mod viz;

pub use history::History;
pub use ui::{Block, Color, Line, Span, Style, Widget};

/// Metadata returned from a plugin's `metadata` export.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct ExtensionMetadata {
    pub id: String,
    pub name: String,
    pub author: String,
    pub version: String,
    pub api_version: String,
    pub description: String,
}

/// `api_version` for extensions that call `vanta_query`.
///
/// The host gate accepts only `0.9*`, so telemetry capability is signalled by
/// the patch component rather than a major bump: `0.9.2` means "requires a
/// host that provides the telemetry host function" (Vanta >= 0.10.26).
pub const API_VERSION_TELEMETRY: &str = "0.9.2";

/// `api_version` for self-contained extensions that import no host function
/// and therefore run on any 0.9-era host.
pub const API_VERSION_BASE: &str = "0.9.0";

impl ExtensionMetadata {
    pub fn new(id: &str, name: &str, version: &str, description: &str, api_version: &str) -> Self {
        ExtensionMetadata {
            id: id.into(),
            name: name.into(),
            author: "zius".into(),
            version: version.into(),
            api_version: api_version.into(),
            description: description.into(),
        }
    }
    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }
}
