//! System health assessment.
//!
//! A deliberately small, explicit rule set. The point of a health panel is to
//! answer "is anything wrong, and what" at a glance — so every verdict names
//! the subsystem, the measured value and the threshold it crossed. No scores,
//! no invented composite index.

use vanta_ext_sdk::telemetry::{Cpu, Disk, Memory};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Ok,
    Warn,
    Critical,
}

impl Level {
    pub fn label(self) -> &'static str {
        match self {
            Level::Ok => "nominal",
            Level::Warn => "degraded",
            Level::Critical => "critical",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Finding {
    pub level: Level,
    pub subsystem: &'static str,
    pub detail: String,
}

/// Thresholds, named so the UI can explain itself.
const CPU_WARN: f64 = 85.0;
const CPU_CRIT: f64 = 95.0;
const MEM_WARN: f64 = 85.0;
const MEM_CRIT: f64 = 95.0;
const SWAP_WARN: f64 = 50.0;
const DISK_WARN: f64 = 85.0;
const DISK_CRIT: f64 = 95.0;
const TEMP_WARN: f64 = 80.0;
const TEMP_CRIT: f64 = 90.0;
/// Load is per-core; 1.0 means fully subscribed.
const LOAD_WARN: f64 = 1.0;
const LOAD_CRIT: f64 = 2.0;

fn grade(
    value: f64,
    warn: f64,
    crit: f64,
    subsystem: &'static str,
    detail: impl Fn(&'static str) -> String,
) -> Option<Finding> {
    if value >= crit {
        Some(Finding {
            level: Level::Critical,
            subsystem,
            detail: detail("critical"),
        })
    } else if value >= warn {
        Some(Finding {
            level: Level::Warn,
            subsystem,
            detail: detail("warn"),
        })
    } else {
        None
    }
}

/// Assess every subsystem we have data for. Missing data yields no finding —
/// absence of telemetry is never reported as health.
pub fn assess(cpu: Option<&Cpu>, mem: Option<&Memory>, disk: Option<&Disk>) -> Vec<Finding> {
    let mut out = Vec::new();

    if let Some(c) = cpu {
        if let Some(f) = grade(c.usage_pct, CPU_WARN, CPU_CRIT, "cpu", |_| {
            format!("{:.0}% utilisation", c.usage_pct)
        }) {
            out.push(f);
        }
        let per_core = c.load1 / c.core_count.max(1) as f64;
        if let Some(f) = grade(per_core, LOAD_WARN, LOAD_CRIT, "load", |_| {
            format!(
                "{:.2} per core ({:.2} over {})",
                per_core, c.load1, c.core_count
            )
        }) {
            out.push(f);
        }
        if let Some(t) = c.max_temp_c {
            if let Some(f) = grade(t, TEMP_WARN, TEMP_CRIT, "thermal", |_| format!("{t:.0}°C")) {
                out.push(f);
            }
        }
    }

    if let Some(m) = mem {
        if let Some(f) = grade(m.used_pct, MEM_WARN, MEM_CRIT, "memory", |_| {
            format!("{:.0}% used", m.used_pct)
        }) {
            out.push(f);
        }
        // Swap pressure matters even at modest percentages: it means the
        // working set no longer fits.
        if m.swap_total_bytes > 0 && m.swap_used_pct >= SWAP_WARN {
            out.push(Finding {
                level: Level::Warn,
                subsystem: "swap",
                detail: format!("{:.0}% of swap in use", m.swap_used_pct),
            });
        }
    }

    if let Some(d) = disk {
        for m in &d.mounts {
            if let Some(f) = grade(m.used_pct, DISK_WARN, DISK_CRIT, "disk", |_| {
                format!("{} at {:.0}%", m.path, m.used_pct)
            }) {
                out.push(f);
            }
        }
    }

    // Worst first: the panel truncates at the bottom on short terminals.
    out.sort_by_key(|f| std::cmp::Reverse(f.level));
    out
}

/// Overall level = the worst finding.
pub fn overall(findings: &[Finding]) -> Level {
    findings.iter().map(|f| f.level).max().unwrap_or(Level::Ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cpu(usage: f64, load1: f64, cores: usize, temp: Option<f64>) -> Cpu {
        Cpu {
            usage_pct: usage,
            cores: vec![usage; cores],
            core_count: cores,
            load1,
            load5: load1,
            load15: load1,
            freq_mhz: 2000,
            temps_c: temp.map(|t| vec![t]).unwrap_or_default(),
            max_temp_c: temp,
        }
    }

    fn mem(used_pct: f64, swap_pct: f64) -> Memory {
        Memory {
            total_bytes: 100,
            used_bytes: used_pct as u64,
            used_pct,
            swap_total_bytes: 100,
            swap_used_bytes: swap_pct as u64,
            swap_used_pct: swap_pct,
        }
    }

    #[test]
    fn healthy_system_has_no_findings() {
        let f = assess(
            Some(&cpu(10.0, 0.5, 8, Some(45.0))),
            Some(&mem(30.0, 0.0)),
            None,
        );
        assert!(f.is_empty());
        assert_eq!(overall(&f), Level::Ok);
    }

    #[test]
    fn missing_telemetry_is_not_reported_as_health() {
        let f = assess(None, None, None);
        assert!(f.is_empty(), "absent data must not produce findings");
        assert_eq!(overall(&f), Level::Ok);
    }

    #[test]
    fn load_is_judged_per_core_not_absolute() {
        // 8.0 load on 8 cores is fully subscribed but not critical.
        let f = assess(Some(&cpu(10.0, 8.0, 8, None)), None, None);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].subsystem, "load");
        assert_eq!(f[0].level, Level::Warn);
        // The same load on 2 cores is critical.
        let f = assess(Some(&cpu(10.0, 8.0, 2, None)), None, None);
        assert_eq!(f[0].level, Level::Critical);
    }

    #[test]
    fn thresholds_escalate_warn_to_critical() {
        assert_eq!(
            assess(Some(&cpu(90.0, 0.0, 8, None)), None, None)[0].level,
            Level::Warn
        );
        assert_eq!(
            assess(Some(&cpu(99.0, 0.0, 8, None)), None, None)[0].level,
            Level::Critical
        );
    }

    #[test]
    fn swap_pressure_is_flagged_below_memory_thresholds() {
        let f = assess(None, Some(&mem(40.0, 60.0)), None);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].subsystem, "swap");
    }

    #[test]
    fn worst_finding_sorts_first_and_sets_overall() {
        let f = assess(
            Some(&cpu(90.0, 0.0, 8, Some(95.0))),
            Some(&mem(99.0, 0.0)),
            None,
        );
        assert_eq!(f[0].level, Level::Critical);
        assert_eq!(overall(&f), Level::Critical);
    }
}
