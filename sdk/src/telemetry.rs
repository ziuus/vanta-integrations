//! Typed client for the host's `vanta_query` telemetry function.
//!
//! The host function only exists on Vanta >= 0.10.26. Importing it makes the
//! plugin fail to instantiate on older hosts (unknown import), so an extension
//! that uses this module declares `api_version = "0.9.2"`.
//!
//! Every getter returns `Result`, and callers are expected to render
//! `ui::unavailable()` on error rather than substituting invented values.

use serde::Deserialize;

#[cfg(target_arch = "wasm32")]
#[extism_pdk::host_fn]
extern "ExtismHost" {
    fn vanta_query(request: String) -> String;
}

/// Off-wasm (unit tests on the host) there is no Extism host to call. Tests
/// exercise the parsing layer directly via `parse_response`.
///
/// Fully qualified `std::result::Result`: the module-level `Result<T>` alias
/// below would otherwise shadow it and take only one type parameter.
#[cfg(not(target_arch = "wasm32"))]
unsafe fn vanta_query(_request: String) -> std::result::Result<String, extism_pdk::Error> {
    Err(extism_pdk::Error::msg("vanta_query unavailable off-wasm"))
}

#[derive(Debug)]
pub enum TelemetryError {
    /// The host function is missing or failed.
    Host(String),
    /// The host answered `ok: false` (usually an unknown topic).
    Refused(String),
    /// The response did not match the expected shape.
    Decode(String),
}

impl std::fmt::Display for TelemetryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TelemetryError::Host(e) => write!(f, "host unavailable: {e}"),
            TelemetryError::Refused(e) => write!(f, "{e}"),
            TelemetryError::Decode(e) => write!(f, "malformed response: {e}"),
        }
    }
}

pub type Result<T> = std::result::Result<T, TelemetryError>;

/// Split out from the transport so it can be unit-tested without a host.
pub fn parse_response<T: for<'de> Deserialize<'de>>(raw: &str) -> Result<T> {
    let v: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| TelemetryError::Decode(e.to_string()))?;
    if v.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
        let msg = v
            .get("error")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("query refused")
            .to_string();
        return Err(TelemetryError::Refused(msg));
    }
    let data = v
        .get("data")
        .ok_or_else(|| TelemetryError::Decode("missing data".into()))?;
    serde_json::from_value(data.clone()).map_err(|e| TelemetryError::Decode(e.to_string()))
}

fn query<T: for<'de> Deserialize<'de>>(request: &str) -> Result<T> {
    let raw = unsafe { vanta_query(request.to_string()) }
        .map_err(|e| TelemetryError::Host(e.to_string()))?;
    parse_response(&raw)
}

// ── Topic payloads ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct Capabilities {
    pub telemetry_api: String,
    pub host_version: String,
    pub topics: Vec<String>,
    #[serde(default)]
    pub unavailable: Vec<Gap>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Gap {
    pub topic: String,
    pub reason: String,
}

impl Capabilities {
    pub fn has(&self, topic: &str) -> bool {
        self.topics.iter().any(|t| t == topic)
    }
    /// Documented reason a topic is missing, for `ui::unavailable`.
    pub fn reason(&self, topic: &str) -> Option<&str> {
        self.unavailable
            .iter()
            .find(|g| g.topic == topic)
            .map(|g| g.reason.as_str())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Summary {
    pub cpu_pct: f64,
    pub mem_pct: f64,
    pub gpu_pct: Option<f64>,
    pub disk_pct: Option<f64>,
    pub rx_kbps: f64,
    pub tx_kbps: f64,
    pub battery_pct: Option<u8>,
    pub battery_charging: Option<bool>,
    pub uptime: String,
    pub temp_c: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Cpu {
    pub usage_pct: f64,
    pub cores: Vec<f64>,
    pub core_count: usize,
    pub load1: f64,
    pub load5: f64,
    pub load15: f64,
    pub freq_mhz: u64,
    pub temps_c: Vec<f64>,
    pub max_temp_c: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Memory {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub used_pct: f64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub swap_used_pct: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Network {
    pub rx_kbps: f64,
    pub tx_kbps: f64,
    pub rx_total_bytes: u64,
    pub tx_total_bytes: u64,
    #[serde(default)]
    pub aggregate_only: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Disk {
    pub mounts: Vec<Mount>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Mount {
    pub path: String,
    pub device: String,
    pub used_bytes: u64,
    pub total_bytes: u64,
    pub used_pct: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Gpu {
    pub present: bool,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub util_pct: Option<f64>,
    #[serde(default)]
    pub temp_c: Option<f64>,
    #[serde(default)]
    pub mem_used_mb: Option<f64>,
    #[serde(default)]
    pub mem_total_mb: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Processes {
    pub total: usize,
    pub processes: Vec<Process>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Process {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
    #[serde(default)]
    pub cmdline: String,
    pub cpu_pct: f64,
    pub mem_kb: u64,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub threads: u64,
    #[serde(default)]
    pub uid: u32,
    #[serde(default)]
    pub read_bps: f64,
    #[serde(default)]
    pub write_bps: f64,
}

// ── Accessors ─────────────────────────────────────────────────────────────

pub fn capabilities() -> Result<Capabilities> {
    query(r#"{"topic":"capabilities"}"#)
}
pub fn summary() -> Result<Summary> {
    query(r#"{"topic":"summary"}"#)
}
pub fn cpu() -> Result<Cpu> {
    query(r#"{"topic":"cpu"}"#)
}
pub fn memory() -> Result<Memory> {
    query(r#"{"topic":"memory"}"#)
}
pub fn network() -> Result<Network> {
    query(r#"{"topic":"network"}"#)
}
pub fn disk() -> Result<Disk> {
    query(r#"{"topic":"disk"}"#)
}
pub fn gpu() -> Result<Gpu> {
    query(r#"{"topic":"gpu"}"#)
}

/// Top processes by CPU. `limit` is clamped host-side to 200.
pub fn processes(limit: usize) -> Result<Processes> {
    query(&format!(r#"{{"topic":"processes","limit":{limit}}}"#))
}

// ── v1.1 topics ───────────────────────────────────────────────────────────────

/// One entry from the `io` topic — process-level I/O throughput.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct IoProcess {
    pub pid: u32,
    pub name: String,
    /// Bytes read per second (rolling average from `/proc/<pid>/io`).
    #[serde(default)]
    pub read_bps: f64,
    /// Bytes written per second.
    #[serde(default)]
    pub write_bps: f64,
    /// `read_bps + write_bps`, pre-summed by host.
    #[serde(default)]
    pub total_bps: f64,
    #[serde(default)]
    pub cpu_pct: f64,
    #[serde(default)]
    pub mem_kb: u64,
}

/// Response shape for `{topic: "io"}`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct IoSnapshot {
    /// Total read throughput across all returned processes.
    #[serde(default)]
    pub total_read_bps: f64,
    /// Total write throughput across all returned processes.
    #[serde(default)]
    pub total_write_bps: f64,
    pub processes: Vec<IoProcess>,
}

impl IoSnapshot {
    pub fn total_bps(&self) -> f64 {
        self.total_read_bps + self.total_write_bps
    }
}

/// Top processes by I/O throughput. `limit` is clamped host-side to 200.
pub fn io(limit: usize) -> Result<IoSnapshot> {
    query(&format!(r#"{{"topic":"io","limit":{limit}}}"#))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_successful_payload() {
        let raw = r#"{"ok":true,"data":{"total":2,"processes":[
            {"pid":1,"ppid":0,"name":"init","cpu_pct":0.5,"mem_kb":1024}]}}"#;
        let p: Processes = parse_response(raw).unwrap();
        assert_eq!(p.total, 2);
        assert_eq!(p.processes[0].name, "init");
        // Absent optional fields fall back to defaults rather than failing.
        assert_eq!(p.processes[0].threads, 0);
        assert_eq!(p.processes[0].cmdline, "");
    }

    #[test]
    fn refusal_is_surfaced_not_swallowed() {
        let e = parse_response::<Cpu>(r#"{"ok":false,"error":"unknown topic: cpu"}"#).unwrap_err();
        match e {
            TelemetryError::Refused(m) => assert!(m.contains("unknown topic")),
            other => panic!("expected Refused, got {other:?}"),
        }
    }

    #[test]
    fn malformed_payloads_are_errors_never_panics() {
        assert!(parse_response::<Cpu>("not json").is_err());
        assert!(parse_response::<Cpu>("{}").is_err());
        assert!(parse_response::<Cpu>(r#"{"ok":true}"#).is_err());
        assert!(parse_response::<Cpu>(r#"{"ok":true,"data":{"usage_pct":"x"}}"#).is_err());
    }

    #[test]
    fn capabilities_expose_documented_gaps() {
        let raw = r#"{"ok":true,"data":{"telemetry_api":"1.0","host_version":"0.10.26",
            "topics":["cpu","processes"],
            "unavailable":[{"topic":"network.connections","reason":"not collected"}]}}"#;
        let c: Capabilities = parse_response(raw).unwrap();
        assert!(c.has("cpu"));
        assert!(!c.has("containers"));
        assert_eq!(c.reason("network.connections"), Some("not collected"));
        assert_eq!(c.reason("cpu"), None);
    }
}
