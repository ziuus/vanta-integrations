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

/// Owns the engine and the last-known telemetry error.
pub struct Watcher {
    pub engine: Engine,
    /// Set when the last evaluation could not read telemetry at all.
    pub telemetry_error: Option<String>,
    /// Wall-clock ms of the last ingested sample, for staleness display.
    pub last_sample_ms: u64,
}

impl Watcher {
    pub fn new() -> Self {
        Watcher {
            engine: Engine::new(Config::default()),
            telemetry_error: None,
            last_sample_ms: 0,
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
        self.engine.ingest(now_ms, &obs, ctx.as_deref());
        self.last_sample_ms = now_ms;
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
    fn telemetry_disappearing_does_not_open_an_incident() {
        let mut w = Watcher::new();
        for i in 0..20 {
            w.ingest(i * 500, &Sample::default());
        }
        assert_eq!(w.engine.active_count(), 0);
        assert_eq!(w.engine.closed().count(), 0);
    }
}
