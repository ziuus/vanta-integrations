//! Turns a telemetry snapshot into breach observations, and owns the engine.
//!
//! Split from `incident` so the lifecycle logic never touches telemetry
//! types, and split from the widgets so rendering only reads state.
//!
//! Threshold pairs are (trigger, clear). The gap between them is the
//! hysteresis band: once an incident is open the value must fall below
//! `clear` to start recovering, so a metric hovering at the trigger cannot
//! oscillate. Values are constants because the host passes no configuration
//! to plugins (see docs §1.3).

use crate::health::Level;
use crate::incident::{Config, Engine, Observation, ProcSample};
use vanta_ext_sdk::telemetry::{Cpu, Disk, Memory, Process};

/// (warn trigger, critical trigger, clear) for percentage metrics.
const CPU: (f64, f64, f64) = (85.0, 95.0, 75.0);
const MEM: (f64, f64, f64) = (85.0, 95.0, 78.0);
const SWAP: (f64, f64, f64) = (50.0, 80.0, 40.0);
const DISK: (f64, f64, f64) = (85.0, 95.0, 82.0);
/// Degrees celsius.
const TEMP: (f64, f64, f64) = (80.0, 90.0, 72.0);
/// Load average per core; 1.0 means fully subscribed.
const LOAD: (f64, f64, f64) = (1.0, 2.0, 0.8);

/// Telemetry needed for one evaluation. Every field optional: absent
/// telemetry must never become a finding.
#[derive(Default)]
pub struct Sample<'a> {
    pub cpu: Option<&'a Cpu>,
    pub memory: Option<&'a Memory>,
    pub disk: Option<&'a Disk>,
    pub processes: Option<&'a [Process]>,
}

fn grade(value: f64, t: (f64, f64, f64)) -> Option<Level> {
    if value >= t.1 {
        Some(Level::Critical)
    } else if value >= t.0 {
        Some(Level::Warn)
    } else {
        None
    }
}

/// Build observations for everything currently in breach.
///
/// A condition is reported while it is above the **clear** threshold *and*
/// an incident already exists for it, or above the **trigger** threshold
/// otherwise. That is what implements hysteresis: `open_keys` tells us which
/// conditions are already active.
pub fn observe(sample: &Sample, open_keys: &[String]) -> Vec<Observation> {
    let mut out = Vec::new();
    let mut check = |key: String, metric: String, value: f64, t: (f64, f64, f64)| {
        let active = open_keys.contains(&key);
        let level = match grade(value, t) {
            Some(l) => Some(l),
            // Already open and still above the clear threshold: keep it open
            // at warn rather than closing the moment it dips under trigger.
            None if active && value >= t.2 => Some(Level::Warn),
            None => None,
        };
        if let Some(level) = level {
            out.push(Observation {
                key,
                metric,
                value,
                threshold: t.0,
                clear_threshold: t.2,
                level,
            });
        }
    };

    if let Some(c) = sample.cpu {
        check("cpu".into(), "cpu utilisation".into(), c.usage_pct, CPU);
        let per_core = c.load1 / c.core_count.max(1) as f64;
        check("load".into(), "load per core".into(), per_core, LOAD);
        if let Some(t) = c.max_temp_c {
            check("thermal".into(), "cpu temperature".into(), t, TEMP);
        }
    }
    if let Some(m) = sample.memory {
        check("memory".into(), "memory used".into(), m.used_pct, MEM);
        if m.swap_total_bytes > 0 {
            check("swap".into(), "swap used".into(), m.swap_used_pct, SWAP);
        }
    }
    if let Some(d) = sample.disk {
        for mount in &d.mounts {
            check(
                format!("disk:{}", mount.path),
                format!("disk {}", mount.path),
                mount.used_pct,
                DISK,
            );
        }
    }
    out
}

/// Units for display: percent metrics render `%`, thermal `°C`, load bare.
pub fn unit_for(key: &str) -> &'static str {
    match key {
        "thermal" => "°C",
        "load" => "",
        _ => "%",
    }
}

/// Samples retained per watched condition. Fixed, because WASM linear
/// memory is never returned to the host.
pub const SIGNAL_LEN: usize = 96;
/// Conditions whose signal is retained. Bounded so a machine with many
/// mounts cannot grow memory without limit.
const MAX_SIGNALS: usize = 12;

/// Rolling signal for one condition, plus the sample offsets where its
/// incident opened and closed. Offsets index the same ring, so a strip and
/// its markers always line up.
#[derive(Default, Clone)]
pub struct Signal {
    values: Vec<f64>,
    /// Sample index (monotonic since start) of each retained value.
    first_index: u64,
    next_index: u64,
    pub opened_at: Option<u64>,
    pub closed_at: Option<u64>,
    pub peak_at: Option<u64>,
    peak_value: f64,
}

impl Signal {
    fn push(&mut self, v: f64) {
        if self.values.len() == SIGNAL_LEN {
            self.values.remove(0);
            self.first_index += 1;
        }
        if self.values.is_empty() || v > self.peak_value {
            self.peak_value = v;
            self.peak_at = Some(self.next_index);
        }
        self.values.push(v);
        self.next_index += 1;
    }

    pub fn values(&self) -> &[f64] {
        &self.values
    }

    /// Convert a sample index into an offset within `values`, if retained.
    pub fn offset_of(&self, index: u64) -> Option<usize> {
        index
            .checked_sub(self.first_index)
            .map(|o| o as usize)
            .filter(|o| *o < self.values.len())
    }

    pub fn current_index(&self) -> u64 {
        self.next_index.saturating_sub(1)
    }
}

/// Owns the engine and the last-known telemetry error.
pub struct Watcher {
    pub engine: Engine,
    /// Set when the last evaluation could not read telemetry at all.
    pub telemetry_error: Option<String>,
    /// Wall-clock ms of the last ingested sample, for staleness display.
    pub last_sample_ms: u64,
    /// Per-condition signal history, keyed by condition key.
    signals: Vec<(String, Signal)>,
}

impl Watcher {
    pub fn new() -> Self {
        Watcher {
            engine: Engine::new(Config::default()),
            telemetry_error: None,
            last_sample_ms: 0,
            signals: Vec::new(),
        }
    }

    /// Evaluate one telemetry sample.
    pub fn ingest(&mut self, now_ms: u64, sample: &Sample) {
        // Conditions already tracked, including those still debouncing:
        // hysteresis must apply to them too, otherwise a condition sitting
        // in the band would be dropped and re-created every sample.
        let keys: Vec<String> = self.engine.tracked_keys();
        let obs = observe(sample, &keys);
        let ctx: Option<Vec<ProcSample>> = sample.processes.map(|ps| {
            ps.iter()
                .map(|p| ProcSample {
                    pid: p.pid,
                    name: p.name.clone(),
                    cpu_pct: p.cpu_pct,
                    mem_kb: p.mem_kb,
                })
                .collect()
        });
        let before: Vec<(String, bool)> = self
            .engine
            .tracked_keys()
            .into_iter()
            .map(|k| {
                let active = self
                    .engine
                    .by_key(&k)
                    .map(|i| i.state.is_active())
                    .unwrap_or(false);
                (k, active)
            })
            .collect();

        self.engine.ingest(now_ms, &obs, ctx.as_deref());
        self.last_sample_ms = now_ms;

        // Record the signal for every measurable condition, breaching or
        // not: the values *before* a breach are what make a signal strip
        // readable, and they are what before/during/after needs.
        for (key, value) in measured(sample) {
            let sig = self.signal_mut(&key);
            sig.push(value);
            let idx = sig.current_index();
            let now_active = self
                .engine
                .by_key(&key)
                .map(|i| i.state.is_active())
                .unwrap_or(false);
            let was_active = before.iter().any(|(k, a)| *k == key && *a);
            if now_active && !was_active {
                let sig = self.signal_mut(&key);
                sig.opened_at = Some(idx);
                sig.closed_at = None;
            } else if was_active && !now_active {
                self.signal_mut(&key).closed_at = Some(idx);
            }
        }
    }

    fn signal_mut(&mut self, key: &str) -> &mut Signal {
        if let Some(pos) = self.signals.iter().position(|(k, _)| k == key) {
            return &mut self.signals[pos].1;
        }
        if self.signals.len() == MAX_SIGNALS {
            self.signals.remove(0);
        }
        self.signals.push((key.to_string(), Signal::default()));
        let last = self.signals.len() - 1;
        &mut self.signals[last].1
    }

    /// Retained signal for a condition, if it has been measured.
    pub fn signal(&self, key: &str) -> Option<&Signal> {
        self.signals.iter().find(|(k, _)| k == key).map(|(_, s)| s)
    }
}

/// Every condition the current sample can measure, with its value. Unlike
/// `observe` this does not filter by threshold — it is what the signal
/// history and the coverage indicator are built from.
pub fn measured(sample: &Sample) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    if let Some(c) = sample.cpu {
        out.push(("cpu".to_string(), c.usage_pct));
        out.push(("load".to_string(), c.load1 / c.core_count.max(1) as f64));
        if let Some(t) = c.max_temp_c {
            out.push(("thermal".to_string(), t));
        }
    }
    if let Some(m) = sample.memory {
        out.push(("memory".to_string(), m.used_pct));
        if m.swap_total_bytes > 0 {
            out.push(("swap".to_string(), m.swap_used_pct));
        }
    }
    if let Some(d) = sample.disk {
        for mount in &d.mounts {
            out.push((format!("disk:{}", mount.path), mount.used_pct));
        }
    }
    out
}

/// (trigger, clear) for a condition key, so gauges can draw both bands.
pub fn thresholds_for(key: &str) -> (f64, f64) {
    let t = match key {
        "cpu" => CPU,
        "load" => LOAD,
        "thermal" => TEMP,
        "memory" => MEM,
        "swap" => SWAP,
        _ => DISK,
    };
    (t.0, t.2)
}

/// Display ceiling for a condition's gauge.
pub fn scale_for(key: &str) -> f64 {
    match key {
        "load" => 2.5,
        _ => 100.0,
    }
}

/// Short human label for a condition key.
pub fn label_for(key: &str) -> String {
    match key {
        "cpu" => "cpu".into(),
        "load" => "load".into(),
        "thermal" => "temp".into(),
        "memory" => "mem".into(),
        "swap" => "swap".into(),
        other => other.strip_prefix("disk:").unwrap_or(other).to_string(),
    }
}

impl Default for Watcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vanta_ext_sdk::telemetry::Mount;

    fn cpu(usage: f64, load1: f64, cores: usize, temp: Option<f64>) -> Cpu {
        Cpu {
            usage_pct: usage,
            cores: vec![],
            core_count: cores,
            load1,
            load5: 0.0,
            load15: 0.0,
            freq_mhz: 0,
            temps_c: vec![],
            max_temp_c: temp,
        }
    }
    fn mem(used: f64, swap: f64, swap_total: u64) -> Memory {
        Memory {
            total_bytes: 100,
            used_bytes: 0,
            used_pct: used,
            swap_total_bytes: swap_total,
            swap_used_bytes: 0,
            swap_used_pct: swap,
        }
    }
    fn disk(pairs: &[(&str, f64)]) -> Disk {
        Disk {
            mounts: pairs
                .iter()
                .map(|(p, pct)| Mount {
                    path: p.to_string(),
                    device: String::new(),
                    used_bytes: 0,
                    total_bytes: 1,
                    used_pct: *pct,
                })
                .collect(),
        }
    }

    #[test]
    fn healthy_sample_produces_no_observations() {
        let c = cpu(10.0, 0.2, 8, Some(40.0));
        let m = mem(30.0, 0.0, 100);
        let d = disk(&[("/", 20.0)]);
        let s = Sample {
            cpu: Some(&c),
            memory: Some(&m),
            disk: Some(&d),
            processes: None,
        };
        assert!(observe(&s, &[]).is_empty());
    }

    #[test]
    fn absent_telemetry_produces_no_observations() {
        assert!(observe(&Sample::default(), &[]).is_empty());
    }

    #[test]
    fn breaching_metrics_are_reported_with_levels() {
        let c = cpu(96.0, 0.1, 8, Some(85.0));
        let s = Sample {
            cpu: Some(&c),
            ..Default::default()
        };
        let o = observe(&s, &[]);
        let by = |k: &str| o.iter().find(|x| x.key == k).unwrap();
        assert_eq!(by("cpu").level, Level::Critical);
        assert_eq!(by("thermal").level, Level::Warn);
        assert!(o.iter().all(|x| x.clear_threshold < x.threshold));
    }

    #[test]
    fn load_is_per_core() {
        let c = cpu(0.0, 8.0, 8, None);
        let s = Sample {
            cpu: Some(&c),
            ..Default::default()
        };
        // 8.0 over 8 cores == 1.0 per core == warn, not critical.
        assert_eq!(
            observe(&s, &[])
                .iter()
                .find(|o| o.key == "load")
                .unwrap()
                .level,
            Level::Warn
        );
        let c = cpu(0.0, 8.0, 2, None);
        let s = Sample {
            cpu: Some(&c),
            ..Default::default()
        };
        assert_eq!(
            observe(&s, &[])
                .iter()
                .find(|o| o.key == "load")
                .unwrap()
                .level,
            Level::Critical
        );
    }

    #[test]
    fn hysteresis_band_keeps_an_open_condition_reported() {
        let c = cpu(80.0, 0.0, 8, None); // below trigger 85, above clear 75
        let s = Sample {
            cpu: Some(&c),
            ..Default::default()
        };
        // Not yet open: nothing reported.
        assert!(observe(&s, &[]).iter().all(|o| o.key != "cpu"));
        // Already open: still reported, so it does not immediately close.
        let open = vec!["cpu".to_string()];
        assert!(observe(&s, &open).iter().any(|o| o.key == "cpu"));
        // Below the clear threshold it finally drops out.
        let c = cpu(70.0, 0.0, 8, None);
        let s = Sample {
            cpu: Some(&c),
            ..Default::default()
        };
        assert!(observe(&s, &open).iter().all(|o| o.key != "cpu"));
    }

    #[test]
    fn swap_is_skipped_when_there_is_no_swap_device() {
        let m = mem(10.0, 99.0, 0);
        let s = Sample {
            memory: Some(&m),
            ..Default::default()
        };
        assert!(observe(&s, &[]).iter().all(|o| o.key != "swap"));
    }

    #[test]
    fn each_mount_is_its_own_condition() {
        let d = disk(&[("/", 96.0), ("/home", 90.0), ("/boot", 10.0)]);
        let s = Sample {
            disk: Some(&d),
            ..Default::default()
        };
        let o = observe(&s, &[]);
        assert_eq!(o.len(), 2);
        assert_eq!(
            o.iter().find(|x| x.key == "disk:/").unwrap().level,
            Level::Critical
        );
        assert_eq!(
            o.iter().find(|x| x.key == "disk:/home").unwrap().level,
            Level::Warn
        );
    }

    #[test]
    fn watcher_opens_and_closes_an_incident_end_to_end() {
        let mut w = Watcher::new();
        let hot = cpu(99.0, 0.0, 8, None);
        let procs = vec![Process {
            pid: 7,
            ppid: 1,
            name: "hog".into(),
            cmdline: "hog --spin".into(),
            cpu_pct: 380.0,
            mem_kb: 2048,
            state: "R".into(),
            threads: 4,
            uid: 1000,
            read_bps: 0.0,
            write_bps: 0.0,
        }];
        let mut t = 0;
        for _ in 0..Config::default().open_after {
            w.ingest(
                t,
                &Sample {
                    cpu: Some(&hot),
                    processes: Some(&procs),
                    ..Default::default()
                },
            );
            t += 500;
        }
        assert_eq!(w.engine.active_count(), 1);
        let inc = w.engine.active().next().unwrap();
        assert_eq!(inc.key, "cpu");
        assert_eq!(inc.context[0].name, "hog");

        let calm = cpu(5.0, 0.0, 8, None);
        for _ in 0..Config::default().close_after {
            w.ingest(
                t,
                &Sample {
                    cpu: Some(&calm),
                    ..Default::default()
                },
            );
            t += 500;
        }
        assert_eq!(w.engine.active_count(), 0);
        assert_eq!(w.engine.closed().next().unwrap().key, "cpu");
    }

    #[test]
    fn signal_history_records_even_when_healthy() {
        let mut w = Watcher::new();
        let calm = cpu(10.0, 0.1, 8, Some(40.0));
        for i in 0..5 {
            w.ingest(
                i * 500,
                &Sample {
                    cpu: Some(&calm),
                    ..Default::default()
                },
            );
        }
        let sig = w.signal("cpu").expect("cpu signal retained");
        assert_eq!(sig.values().len(), 5);
        assert!(sig.opened_at.is_none(), "no incident, no open marker");
        assert_eq!(w.engine.active_count(), 0);
    }

    #[test]
    fn signal_history_is_bounded() {
        let mut w = Watcher::new();
        let calm = cpu(10.0, 0.1, 8, None);
        for i in 0..(SIGNAL_LEN as u64 + 40) {
            w.ingest(
                i * 100,
                &Sample {
                    cpu: Some(&calm),
                    ..Default::default()
                },
            );
        }
        assert_eq!(w.signal("cpu").unwrap().values().len(), SIGNAL_LEN);
    }

    #[test]
    fn signal_marks_open_and_close_offsets() {
        let mut w = Watcher::new();
        let hot = cpu(99.0, 0.1, 8, None);
        let calm = cpu(5.0, 0.1, 8, None);
        let mut t = 0;
        for _ in 0..Config::default().open_after {
            w.ingest(
                t,
                &Sample {
                    cpu: Some(&hot),
                    ..Default::default()
                },
            );
            t += 500;
        }
        let sig = w.signal("cpu").unwrap();
        let opened = sig.opened_at.expect("open marker recorded");
        assert!(sig.offset_of(opened).is_some(), "marker maps into the ring");
        assert!(sig.closed_at.is_none());

        for _ in 0..Config::default().close_after {
            w.ingest(
                t,
                &Sample {
                    cpu: Some(&calm),
                    ..Default::default()
                },
            );
            t += 500;
        }
        let sig = w.signal("cpu").unwrap();
        assert!(sig.closed_at.is_some(), "close marker recorded");
        assert!(sig.peak_at.is_some());
    }

    #[test]
    fn measured_reports_only_available_telemetry() {
        assert!(measured(&Sample::default()).is_empty());
        let c = cpu(10.0, 0.5, 4, None);
        let keys: Vec<String> = measured(&Sample {
            cpu: Some(&c),
            ..Default::default()
        })
        .into_iter()
        .map(|(k, _)| k)
        .collect();
        assert!(keys.contains(&"cpu".to_string()));
        assert!(keys.contains(&"load".to_string()));
        assert!(!keys.contains(&"thermal".to_string()), "no sensor, no key");
    }

    #[test]
    fn thresholds_are_exposed_with_clear_below_trigger() {
        for k in ["cpu", "load", "thermal", "memory", "swap", "disk:/"] {
            let (trigger, clear) = thresholds_for(k);
            assert!(clear < trigger, "{k}: clear must be below trigger");
            assert!(scale_for(k) > 0.0);
        }
    }

    #[test]
    fn telemetry_disappearing_does_not_open_an_incident() {
        let mut w = Watcher::new();
        for i in 0..20 {
            w.ingest(i * 500, &Sample::default());
        }
        assert_eq!(w.engine.active_count(), 0);
        assert_eq!(w.engine.closed().count(), 0);
    }
}
