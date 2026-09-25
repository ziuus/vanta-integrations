//! Shared telemetry state for all widgets in this extension.
//!
//! Every widget renders from one cached snapshot rather than querying the
//! host itself. Two reasons:
//!
//! * The host calls `render_widget` once **per widget, per frame** with a
//!   10 ms budget each. Five widgets querying six topics would be 30 host
//!   round-trips per frame for data that changes far slower than the frame
//!   rate.
//! * Widgets must agree. Two panels showing different CPU numbers in the same
//!   frame looks broken, and would happen if each sampled independently.
//!
//! The cache TTL is deliberately shorter than the host sampler interval, so
//! the UI never lags behind real data, and history is throttled separately
//! (see `vanta_ext_sdk::History`).

use std::cell::RefCell;

use vanta_ext_sdk::history::{now_ms, History};
use vanta_ext_sdk::telemetry::{self, Capabilities, Cpu, Disk, Memory, Network, Processes};

/// Samples retained per metric. At one sample/second this is four minutes of
/// history — more than any terminal panel can draw, and fixed size.
pub const HIST: usize = 240;
/// Minimum spacing between retained history samples.
const HIST_INTERVAL_MS: u64 = 1000;
/// How long a telemetry read is reused across widgets within a frame.
const CACHE_TTL_MS: u64 = 250;

#[derive(Default)]
pub struct Latest {
    pub cpu: Option<Cpu>,
    pub memory: Option<Memory>,
    pub network: Option<Network>,
    pub disk: Option<Disk>,
    pub processes: Option<Processes>,
    /// Error text from the most recent failed read, shown instead of numbers.
    pub error: Option<String>,
}

pub struct State {
    pub latest: Latest,
    pub caps: Option<Capabilities>,
    pub caps_checked: bool,
    fetched_ms: u64,
    pub cpu_h: History<HIST>,
    pub mem_h: History<HIST>,
    pub rx_h: History<HIST>,
    pub tx_h: History<HIST>,
    pub load_h: History<HIST>,
    pub disk_h: History<HIST>,
}

impl State {
    const fn new() -> Self {
        State {
            latest: Latest {
                cpu: None,
                memory: None,
                network: None,
                disk: None,
                processes: None,
                error: None,
            },
            caps: None,
            caps_checked: false,
            fetched_ms: 0,
            cpu_h: History::new(HIST_INTERVAL_MS),
            mem_h: History::new(HIST_INTERVAL_MS),
            rx_h: History::new(HIST_INTERVAL_MS),
            tx_h: History::new(HIST_INTERVAL_MS),
            load_h: History::new(HIST_INTERVAL_MS),
            disk_h: History::new(HIST_INTERVAL_MS),
        }
    }
}

thread_local! {
    static STATE: RefCell<State> = const { RefCell::new(State::new()) };
}

/// Refresh the cache if stale, then hand the state to `f`.
///
/// WASM plugins are single-threaded, so `thread_local` + `RefCell` gives
/// interior mutability across host calls without `static mut`.
pub fn with<R>(f: impl FnOnce(&State) -> R) -> R {
    STATE.with(|s| {
        {
            let mut st = s.borrow_mut();
            if now_ms().saturating_sub(st.fetched_ms) >= CACHE_TTL_MS {
                refresh(&mut st);
            }
        }
        f(&s.borrow())
    })
}

fn refresh(st: &mut State) {
    st.fetched_ms = now_ms();

    // Capabilities are fetched once: they describe the host build, which
    // cannot change while the process is running.
    if !st.caps_checked {
        st.caps_checked = true;
        st.caps = telemetry::capabilities().ok();
    }

    let mut first_error: Option<String> = None;
    let mut note = |e: telemetry::TelemetryError| {
        if first_error.is_none() {
            first_error = Some(e.to_string());
        }
    };

    match telemetry::cpu() {
        Ok(c) => {
            st.cpu_h.push(c.usage_pct);
            // Load is normalised to core count so the graph means the same
            // thing on a 4-core laptop and a 64-core server.
            let cores = c.core_count.max(1) as f64;
            st.load_h.push((c.load1 / cores * 100.0).min(400.0));
            st.latest.cpu = Some(c);
        }
        Err(e) => note(e),
    }
    match telemetry::memory() {
        Ok(m) => {
            st.mem_h.push(m.used_pct);
            st.latest.memory = Some(m);
        }
        Err(e) => note(e),
    }
    match telemetry::network() {
        Ok(n) => {
            st.rx_h.push(n.rx_kbps);
            st.tx_h.push(n.tx_kbps);
            st.latest.network = Some(n);
        }
        Err(e) => note(e),
    }
    match telemetry::disk() {
        Ok(d) => {
            if let Some(root) = root_mount(&d) {
                st.disk_h.push(root);
            }
            st.latest.disk = Some(d);
        }
        Err(e) => note(e),
    }
    // A small slice: the observatory only needs activity counts and the
    // busiest few, not a full table.
    match telemetry::processes(8) {
        Ok(p) => st.latest.processes = Some(p),
        Err(e) => note(e),
    }

    st.latest.error = first_error;
}

/// Used % of `/`, falling back to the fullest mount when there is no root
/// (which happens on unusual mount layouts).
pub fn root_mount(d: &Disk) -> Option<f64> {
    d.mounts
        .iter()
        .find(|m| m.path == "/")
        .map(|m| m.used_pct)
        .or_else(|| {
            d.mounts
                .iter()
                .map(|m| m.used_pct)
                .fold(None::<f64>, |acc, v| Some(acc.map_or(v, |a| a.max(v))))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disk(paths: &[(&str, f64)]) -> Disk {
        Disk {
            mounts: paths
                .iter()
                .map(|(p, pct)| telemetry::Mount {
                    path: p.to_string(),
                    device: "dev".into(),
                    used_bytes: 0,
                    total_bytes: 0,
                    used_pct: *pct,
                })
                .collect(),
        }
    }

    #[test]
    fn root_mount_prefers_slash() {
        assert_eq!(
            root_mount(&disk(&[("/home", 90.0), ("/", 10.0)])),
            Some(10.0)
        );
    }

    #[test]
    fn root_mount_falls_back_to_fullest() {
        assert_eq!(
            root_mount(&disk(&[("/home", 90.0), ("/boot", 30.0)])),
            Some(90.0)
        );
        assert_eq!(root_mount(&disk(&[])), None);
    }
}
