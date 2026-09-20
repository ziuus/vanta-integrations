//! Incident lifecycle engine.
//!
//! This is the capability Vanta does not have: native panels answer "what is
//! happening now", this answers "what went wrong, when, for how long, and
//! what was running when it started".
//!
//! # Lifecycle
//!
//! ```text
//!            breach sample                 breach sustained
//!   NORMAL ──────────────► PENDING ───────────────────────► OPEN
//!      ▲                      │ recovery sample               │
//!      │                      │ before open_after             │ value stays
//!      │                      ▼                               │ above clear
//!      │                  (discarded, no incident)            ▼
//!      │                                                  PERSISTING
//!      │        recovery sustained for close_after            │
//!      └──────────────────── CLOSED ◄─────────────────────────┘
//! ```
//!
//! * **PENDING** is the debounce window. A metric must breach for
//!   `open_after` consecutive samples before an incident exists at all, so a
//!   one-sample spike never raises an alert. Nothing is shown to the user
//!   during PENDING.
//! * **OPEN** is an incident that has just been raised. It becomes
//!   **PERSISTING** once it has survived `persist_after` samples — the
//!   distinction is what lets the UI separate "just happened" from "this has
//!   been going on".
//! * **CLOSED** requires the value to fall below a *clear* threshold that is
//!   lower than the trigger threshold (hysteresis), sustained for
//!   `close_after` samples. Without the gap a metric hovering on the
//!   threshold would oscillate open/closed every sample.
//!
//! All timing is driven by an injected `now_ms`, so tests are deterministic
//! and no wall clock is read here.

use crate::health::Level;

/// Bounded history: how many closed incidents are retained. Linear memory in
/// a WASM plugin is never reclaimed, so this must be fixed.
pub const MAX_CLOSED: usize = 32;
/// Bounded event feed.
pub const MAX_EVENTS: usize = 64;
/// Processes captured at the moment an incident opens.
pub const MAX_CONTEXT_PROCS: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Debouncing: breaching but not yet an incident. Not user-visible.
    Pending,
    /// Raised, recently.
    Open,
    /// Raised and sustained.
    Persisting,
    /// Recovered.
    Closed,
}

impl State {
    pub fn label(self) -> &'static str {
        match self {
            State::Pending => "pending",
            State::Open => "open",
            State::Persisting => "persisting",
            State::Closed => "closed",
        }
    }
    /// Pending incidents are internal bookkeeping, not findings.
    pub fn is_visible(self) -> bool {
        !matches!(self, State::Pending)
    }
    pub fn is_active(self) -> bool {
        matches!(self, State::Open | State::Persisting)
    }
}

/// A process as it appeared when an incident opened. Deliberately a snapshot
/// copy, not a live lookup: the point is what was running *then*.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcSample {
    pub pid: u32,
    pub name: String,
    pub cpu_pct: f64,
    pub mem_kb: u64,
}

#[derive(Debug, Clone)]
pub struct Incident {
    pub id: u64,
    /// Stable key for the watched condition, e.g. `cpu` or `disk:/home`.
    pub key: String,
    /// Human label for the metric, e.g. `cpu utilisation`.
    pub metric: String,
    pub level: Level,
    /// Value that triggered the incident.
    pub threshold: f64,
    /// Value it must fall below to recover (hysteresis).
    pub clear_threshold: f64,
    pub opened_ms: u64,
    pub closed_ms: Option<u64>,
    /// Most recent observation.
    pub current: f64,
    /// Worst observation seen during the incident.
    pub peak: f64,
    pub state: State,
    /// Samples observed while breaching, including the debounce window.
    pub samples: u32,
    /// Consecutive recovery samples seen so far.
    recovery_samples: u32,
    /// Top processes at the moment the incident opened. Empty when process
    /// telemetry was unavailable then — never backfilled later, because a
    /// later snapshot would not describe the cause.
    pub context: Vec<ProcSample>,
    /// True when process telemetry was unavailable at open time, so the UI
    /// can say so rather than implying nothing was running.
    pub context_unavailable: bool,
}

impl Incident {
    /// Milliseconds the incident has been open. Frozen once closed.
    pub fn duration_ms(&self, now_ms: u64) -> u64 {
        self.closed_ms
            .unwrap_or(now_ms)
            .saturating_sub(self.opened_ms)
    }
}

/// Something worth putting on a feed. Emitted only on real transitions.
#[derive(Debug, Clone, PartialEq)]
pub enum EventKind {
    Opened,
    Escalated,
    Persisting,
    Closed,
}

impl EventKind {
    pub fn label(self) -> &'static str {
        match self {
            EventKind::Opened => "opened",
            EventKind::Escalated => "escalated",
            EventKind::Persisting => "persisting",
            EventKind::Closed => "closed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Event {
    pub at_ms: u64,
    pub kind: EventKind,
    pub incident_id: u64,
    pub key: String,
    pub metric: String,
    pub level: Level,
    pub value: f64,
    /// Present on `Closed` so the feed can show how long it lasted.
    pub duration_ms: Option<u64>,
}

/// One observation of a watched condition for a single sample.
#[derive(Debug, Clone)]
pub struct Observation {
    pub key: String,
    pub metric: String,
    pub value: f64,
    pub threshold: f64,
    pub clear_threshold: f64,
    pub level: Level,
}

#[derive(Debug, Clone, Copy)]
pub struct Config {
    /// Consecutive breaching samples before an incident is raised.
    pub open_after: u32,
    /// Samples after opening before it counts as sustained.
    pub persist_after: u32,
    /// Consecutive recovery samples before closing.
    pub close_after: u32,
}

impl Default for Config {
    fn default() -> Self {
        // At the host's default ~0.5 s sampling these are roughly: raise
        // after ~1.5 s of breach, "sustained" after ~8 s, close after ~3 s
        // back under the clear threshold.
        Config {
            open_after: 3,
            persist_after: 15,
            close_after: 6,
        }
    }
}

pub struct Engine {
    cfg: Config,
    next_id: u64,
    /// Active or debouncing incidents, newest last.
    active: Vec<Incident>,
    closed: Vec<Incident>,
    events: Vec<Event>,
    /// Samples ingested since start; used to report readiness in the UI.
    pub sample_count: u64,
}

impl Engine {
    pub fn new(cfg: Config) -> Self {
        Engine {
            cfg,
            next_id: 1,
            active: Vec::new(),
            closed: Vec::new(),
            events: Vec::new(),
            sample_count: 0,
        }
    }

    /// Incidents the user should see: raised, not yet closed.
    pub fn active(&self) -> impl Iterator<Item = &Incident> {
        self.active.iter().filter(|i| i.state.is_active())
    }

    pub fn active_count(&self) -> usize {
        self.active().count()
    }

    /// Closed incidents, newest first.
    pub fn closed(&self) -> impl Iterator<Item = &Incident> {
        self.closed.iter().rev()
    }

    /// Events, newest first.
    pub fn events(&self) -> impl Iterator<Item = &Event> {
        self.events.iter().rev()
    }

    pub fn worst_active_level(&self) -> Option<Level> {
        self.active().map(|i| i.level).max()
    }

    /// Find an active or pending incident by condition key.
    pub fn by_key(&self, key: &str) -> Option<&Incident> {
        self.active.iter().find(|i| i.key == key)
    }

    fn push_event(&mut self, e: Event) {
        if self.events.len() == MAX_EVENTS {
            self.events.remove(0);
        }
        self.events.push(e);
    }

    /// Ingest one sample.
    ///
    /// `observations` are the conditions currently in breach. A condition
    /// absent from the list is treated as recovered — which is why callers
    /// must omit conditions whose telemetry is *missing* rather than
    /// reporting them as healthy; see `Sentinel::ingest`.
    ///
    /// `top_procs` is the process context to attach if an incident opens on
    /// this sample. `None` means process telemetry was unavailable.
    pub fn ingest(
        &mut self,
        now_ms: u64,
        observations: &[Observation],
        top_procs: Option<&[ProcSample]>,
    ) {
        self.sample_count += 1;

        // 1. Update or create entries for everything currently breaching.
        for obs in observations {
            match self.active.iter_mut().position(|i| i.key == obs.key) {
                Some(idx) => {
                    let (id, transition) = {
                        let inc = &mut self.active[idx];
                        inc.current = obs.value;
                        inc.peak = inc.peak.max(obs.value);
                        inc.samples += 1;
                        inc.recovery_samples = 0;

                        let escalated = obs.level > inc.level;
                        if escalated {
                            inc.level = obs.level;
                            inc.threshold = obs.threshold;
                            inc.clear_threshold = obs.clear_threshold;
                        }

                        let mut transition = None;
                        match inc.state {
                            State::Pending if inc.samples >= self.cfg.open_after => {
                                inc.state = State::Open;
                                inc.opened_ms = now_ms;
                                transition = Some(EventKind::Opened);
                            }
                            State::Open
                                if inc.samples
                                    >= self.cfg.open_after + self.cfg.persist_after =>
                            {
                                inc.state = State::Persisting;
                                transition = Some(EventKind::Persisting);
                            }
                            _ if escalated && inc.state.is_visible() => {
                                transition = Some(EventKind::Escalated);
                            }
                            _ => {}
                        }
                        (inc.id, transition)
                    };

                    // Context is captured exactly when the incident opens.
                    if transition == Some(EventKind::Opened) {
                        let (ctx, missing) = match top_procs {
                            Some(p) => (p.iter().take(MAX_CONTEXT_PROCS).cloned().collect(), false),
                            None => (Vec::new(), true),
                        };
                        let inc = &mut self.active[idx];
                        inc.context = ctx;
                        inc.context_unavailable = missing;
                    }

                    if let Some(kind) = transition {
                        let inc = &self.active[idx];
                        let ev = Event {
                            at_ms: now_ms,
                            kind,
                            incident_id: id,
                            key: inc.key.clone(),
                            metric: inc.metric.clone(),
                            level: inc.level,
                            value: inc.current,
                            duration_ms: None,
                        };
                        self.push_event(ev);
                    }
                }
                None => {
                    // New condition: starts in the debounce window.
                    let id = self.next_id;
                    self.next_id += 1;
                    self.active.push(Incident {
                        id,
                        key: obs.key.clone(),
                        metric: obs.metric.clone(),
                        level: obs.level,
                        threshold: obs.threshold,
                        clear_threshold: obs.clear_threshold,
                        opened_ms: now_ms,
                        closed_ms: None,
                        current: obs.value,
                        peak: obs.value,
                        state: State::Pending,
                        samples: 1,
                        recovery_samples: 0,
                        context: Vec::new(),
                        context_unavailable: false,
                    });
                    // open_after == 1 means no debounce: open immediately.
                    if self.cfg.open_after <= 1 {
                        let idx = self.active.len() - 1;
                        let (ctx, missing) = match top_procs {
                            Some(p) => (p.iter().take(MAX_CONTEXT_PROCS).cloned().collect(), false),
                            None => (Vec::new(), true),
                        };
                        let inc = &mut self.active[idx];
                        inc.state = State::Open;
                        inc.opened_ms = now_ms;
                        inc.context = ctx;
                        inc.context_unavailable = missing;
                        let ev = Event {
                            at_ms: now_ms,
                            kind: EventKind::Opened,
                            incident_id: id,
                            key: obs.key.clone(),
                            metric: obs.metric.clone(),
                            level: obs.level,
                            value: obs.value,
                            duration_ms: None,
                        };
                        self.push_event(ev);
                    }
                }
            }
        }

        // 2. Anything not in this sample's observations is recovering.
        let mut closed_now: Vec<Incident> = Vec::new();
        let close_after = self.cfg.close_after;
        let mut i = 0;
        while i < self.active.len() {
            let still_breaching = observations.iter().any(|o| o.key == self.active[i].key);
            if still_breaching {
                i += 1;
                continue;
            }
            let inc = &mut self.active[i];
            inc.recovery_samples += 1;
            if inc.recovery_samples < close_after {
                i += 1;
                continue;
            }
            // Debounced-away: never became visible, so it leaves no trace.
            if inc.state == State::Pending {
                self.active.remove(i);
                continue;
            }
            inc.state = State::Closed;
            inc.closed_ms = Some(now_ms);
            closed_now.push(self.active.remove(i));
        }

        for inc in closed_now {
            let ev = Event {
                at_ms: now_ms,
                kind: EventKind::Closed,
                incident_id: inc.id,
                key: inc.key.clone(),
                metric: inc.metric.clone(),
                level: inc.level,
                value: inc.current,
                duration_ms: Some(inc.duration_ms(now_ms)),
            };
            self.push_event(ev);
            if self.closed.len() == MAX_CLOSED {
                self.closed.remove(0);
            }
            self.closed.push(inc);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(key: &str, value: f64, level: Level) -> Observation {
        Observation {
            key: key.into(),
            metric: format!("{key} utilisation"),
            value,
            threshold: 85.0,
            clear_threshold: 75.0,
            level,
        }
    }

    fn procs() -> Vec<ProcSample> {
        vec![
            ProcSample {
                pid: 10,
                name: "hog".into(),
                cpu_pct: 190.0,
                mem_kb: 1024,
            },
            ProcSample {
                pid: 11,
                name: "idle".into(),
                cpu_pct: 1.0,
                mem_kb: 512,
            },
        ]
    }

    fn engine() -> Engine {
        Engine::new(Config {
            open_after: 3,
            persist_after: 5,
            close_after: 2,
        })
    }

    /// Feed n breaching samples, 1000 ms apart, starting at `t0`.
    fn breach(e: &mut Engine, t0: u64, n: u32, value: f64, p: Option<&[ProcSample]>) -> u64 {
        let mut t = t0;
        for _ in 0..n {
            e.ingest(t, &[obs("cpu", value, Level::Warn)], p);
            t += 1000;
        }
        t
    }

    fn recover(e: &mut Engine, t0: u64, n: u32) -> u64 {
        let mut t = t0;
        for _ in 0..n {
            e.ingest(t, &[], None);
            t += 1000;
        }
        t
    }

    #[test]
    fn starts_deterministically_empty() {
        let e = engine();
        assert_eq!(e.active_count(), 0);
        assert_eq!(e.closed().count(), 0);
        assert_eq!(e.events().count(), 0);
        assert_eq!(e.sample_count, 0);
        assert_eq!(e.worst_active_level(), None);
    }

    #[test]
    fn debounce_prevents_false_incident() {
        let mut e = engine();
        // Two breaching samples, below open_after = 3.
        breach(&mut e, 0, 2, 90.0, Some(&procs()));
        assert_eq!(e.active_count(), 0, "must not raise inside debounce window");
        assert_eq!(e.events().count(), 0, "no event during debounce");

        // Recovers before opening: leaves no trace at all.
        recover(&mut e, 2000, 2);
        assert_eq!(e.active_count(), 0);
        assert_eq!(e.closed().count(), 0, "debounced spike must not be recorded");
        assert_eq!(e.events().count(), 0);
    }

    #[test]
    fn normal_to_open_after_sustained_breach() {
        let mut e = engine();
        breach(&mut e, 1_000, 3, 90.0, Some(&procs()));
        assert_eq!(e.active_count(), 1);
        let inc = e.active().next().unwrap();
        assert_eq!(inc.state, State::Open);
        // Opened at the sample that crossed the debounce, not at first breach.
        assert_eq!(inc.opened_ms, 3_000);
        assert_eq!(e.events().count(), 1);
        assert_eq!(e.events().next().unwrap().kind, EventKind::Opened);
    }

    #[test]
    fn open_to_persisting_then_closed() {
        let mut e = engine();
        let t = breach(&mut e, 0, 3, 90.0, Some(&procs()));
        assert_eq!(e.active().next().unwrap().state, State::Open);

        // open_after + persist_after = 8 breaching samples total.
        let t = breach(&mut e, t, 5, 90.0, Some(&procs()));
        assert_eq!(e.active().next().unwrap().state, State::Persisting);
        assert!(e.events().any(|ev| ev.kind == EventKind::Persisting));

        // One recovery sample is not enough (close_after = 2).
        e.ingest(t, &[], None);
        assert_eq!(e.active_count(), 1, "single recovery sample must not close");

        e.ingest(t + 1000, &[], None);
        assert_eq!(e.active_count(), 0);
        assert_eq!(e.closed().count(), 1);
        let inc = e.closed().next().unwrap();
        assert_eq!(inc.state, State::Closed);
        assert_eq!(inc.closed_ms, Some(t + 1000));
    }

    #[test]
    fn duration_tracks_open_time_and_freezes_on_close() {
        let mut e = engine();
        // Opens at t=2000 (3rd sample, 0-indexed 1000ms apart).
        let t = breach(&mut e, 0, 3, 90.0, Some(&procs()));
        let opened = e.active().next().unwrap().opened_ms;
        assert_eq!(opened, 2_000);
        assert_eq!(e.active().next().unwrap().duration_ms(5_000), 3_000);

        let t = recover(&mut e, t, 2);
        let inc = e.closed().next().unwrap();
        let closed_at = inc.closed_ms.unwrap();
        let frozen = inc.duration_ms(closed_at);
        // Frozen: querying much later must not grow it.
        assert_eq!(inc.duration_ms(t + 999_000), frozen);
        assert_eq!(frozen, closed_at - opened);
    }

    #[test]
    fn hysteresis_clear_threshold_is_below_trigger() {
        let mut e = engine();
        breach(&mut e, 0, 3, 90.0, Some(&procs()));
        let inc = e.active().next().unwrap();
        assert!(
            inc.clear_threshold < inc.threshold,
            "recovery threshold must be lower than trigger to avoid flapping"
        );
    }

    #[test]
    fn value_between_clear_and_trigger_keeps_incident_open() {
        // The caller reports an observation while value > clear_threshold,
        // so an incident hovering in the hysteresis band stays open instead
        // of oscillating.
        let mut e = engine();
        let t = breach(&mut e, 0, 3, 90.0, Some(&procs()));
        for i in 0..5 {
            e.ingest(t + i * 1000, &[obs("cpu", 80.0, Level::Warn)], None);
        }
        assert_eq!(e.active_count(), 1, "must not flap while in hysteresis band");
        assert_eq!(
            e.events().filter(|ev| ev.kind == EventKind::Closed).count(),
            0
        );
    }

    #[test]
    fn process_context_is_captured_at_open_and_never_rewritten() {
        let mut e = engine();
        let at_open = procs();
        let t = breach(&mut e, 0, 3, 90.0, Some(&at_open));

        let inc = e.active().next().unwrap();
        assert_eq!(inc.context.len(), 2);
        assert_eq!(inc.context[0].name, "hog");
        assert!(!inc.context_unavailable);

        // Later samples carry entirely different processes.
        let later = vec![ProcSample {
            pid: 99,
            name: "unrelated".into(),
            cpu_pct: 5.0,
            mem_kb: 1,
        }];
        breach(&mut e, t, 4, 90.0, Some(&later));
        let inc = e.active().next().unwrap();
        assert_eq!(
            inc.context[0].name, "hog",
            "context must describe the opening moment, not the latest sample"
        );
    }

    #[test]
    fn context_survives_into_the_closed_record() {
        let mut e = engine();
        let t = breach(&mut e, 0, 3, 90.0, Some(&procs()));
        recover(&mut e, t, 2);
        let inc = e.closed().next().unwrap();
        assert_eq!(inc.context[0].name, "hog");
        assert_eq!(inc.context[0].pid, 10);
    }

    #[test]
    fn missing_process_telemetry_is_marked_not_faked() {
        let mut e = engine();
        breach(&mut e, 0, 3, 90.0, None);
        let inc = e.active().next().unwrap();
        assert!(inc.context.is_empty());
        assert!(
            inc.context_unavailable,
            "absent context must be reported, not silently shown as empty"
        );
    }

    #[test]
    fn context_is_bounded() {
        let many: Vec<ProcSample> = (0..50)
            .map(|i| ProcSample {
                pid: i,
                name: format!("p{i}"),
                cpu_pct: 1.0,
                mem_kb: 1,
            })
            .collect();
        let mut e = engine();
        breach(&mut e, 0, 3, 90.0, Some(&many));
        assert_eq!(e.active().next().unwrap().context.len(), MAX_CONTEXT_PROCS);
    }

    #[test]
    fn peak_tracks_worst_value_not_latest() {
        let mut e = engine();
        let mut t = 0;
        for v in [90.0, 99.0, 88.0, 86.0] {
            e.ingest(t, &[obs("cpu", v, Level::Warn)], None);
            t += 1000;
        }
        let inc = e.active().next().unwrap();
        assert_eq!(inc.peak, 99.0);
        assert_eq!(inc.current, 86.0);
    }

    #[test]
    fn escalation_raises_level_and_emits_event() {
        let mut e = engine();
        breach(&mut e, 0, 3, 90.0, None);
        assert_eq!(e.active().next().unwrap().level, Level::Warn);
        e.ingest(3_000, &[obs("cpu", 99.0, Level::Critical)], None);
        assert_eq!(e.active().next().unwrap().level, Level::Critical);
        assert!(e.events().any(|ev| ev.kind == EventKind::Escalated));
        assert_eq!(e.worst_active_level(), Some(Level::Critical));
    }

    #[test]
    fn multiple_incidents_are_tracked_independently() {
        let mut e = engine();
        for i in 0..3 {
            e.ingest(
                i * 1000,
                &[
                    obs("cpu", 90.0, Level::Warn),
                    obs("disk:/", 97.0, Level::Critical),
                ],
                None,
            );
        }
        assert_eq!(e.active_count(), 2);
        assert_eq!(e.worst_active_level(), Some(Level::Critical));

        // Only cpu recovers; disk must remain open.
        for i in 3..5 {
            e.ingest(i * 1000, &[obs("disk:/", 97.0, Level::Critical)], None);
        }
        assert_eq!(e.active_count(), 1);
        assert_eq!(e.active().next().unwrap().key, "disk:/");
        assert_eq!(e.closed().next().unwrap().key, "cpu");
    }

    #[test]
    fn closed_incidents_and_events_stay_bounded() {
        let mut e = Engine::new(Config {
            open_after: 1,
            persist_after: 1,
            close_after: 1,
        });
        let mut t = 0;
        // Each cycle: open one incident then close it.
        for i in 0..(MAX_CLOSED + 20) {
            e.ingest(t, &[obs(&format!("k{i}"), 90.0, Level::Warn)], None);
            t += 1000;
            e.ingest(t, &[], None);
            t += 1000;
        }
        assert_eq!(e.closed().count(), MAX_CLOSED, "closed history must be bounded");
        assert!(e.events().count() <= MAX_EVENTS, "event feed must be bounded");
        // Newest first, and the oldest were evicted.
        assert_eq!(e.closed().next().unwrap().key, format!("k{}", MAX_CLOSED + 19));
    }

    #[test]
    fn no_observations_means_no_incidents() {
        let mut e = engine();
        for i in 0..10 {
            e.ingest(i * 1000, &[], None);
        }
        assert_eq!(e.active_count(), 0);
        assert_eq!(e.closed().count(), 0);
        assert_eq!(e.events().count(), 0);
        assert_eq!(e.sample_count, 10);
    }

    #[test]
    fn ids_are_unique_and_stable_across_incidents() {
        let mut e = Engine::new(Config {
            open_after: 1,
            persist_after: 10,
            close_after: 1,
        });
        e.ingest(0, &[obs("cpu", 90.0, Level::Warn)], None);
        let first = e.active().next().unwrap().id;
        e.ingest(1000, &[], None);
        e.ingest(2000, &[obs("cpu", 90.0, Level::Warn)], None);
        let second = e.active().next().unwrap().id;
        assert_ne!(first, second, "a re-opened condition is a new incident");
        assert_eq!(e.closed().next().unwrap().id, first);
    }
}
