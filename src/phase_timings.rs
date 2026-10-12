use std::{io::Write, time::Instant};

/// Opt-in sequential timings. Nested scopes overlap and are reported separately.
pub struct PhaseTimings {
    active: Option<Active>,
}

struct Active {
    scope: &'static str,
    label: Option<String>,
    started: Instant,
    checkpoint: Instant,
    /// Live heap bytes at the checkpoint (0 unless built with `live_bytes`).
    live: isize,
    phases: Vec<(&'static str, f64, isize)>,
}

/// `RUST_DIALYZER_TIMINGS`: unset/0 off, 1 phase scopes, 2 also per-SCC and
/// per-publication scopes (tens of thousands of records; distorts totals).
fn level() -> u8 {
    static LEVEL: std::sync::OnceLock<u8> = std::sync::OnceLock::new();
    *LEVEL.get_or_init(|| {
        std::env::var("RUST_DIALYZER_TIMINGS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    })
}

impl PhaseTimings {
    pub fn new(scope: &'static str) -> Self {
        Self::at_level(scope, 1)
    }

    /// A scope that runs once per SCC or publication; recorded only at level 2.
    pub fn detail(scope: &'static str) -> Self {
        Self::at_level(scope, 2)
    }

    fn at_level(scope: &'static str, required: u8) -> Self {
        let active = if level() >= required {
            let started = Instant::now();
            Some(Active {
                scope,
                label: None,
                started,
                checkpoint: started,
                live: crate::live_bytes::live(),
                phases: Vec::new(),
            })
        } else {
            None
        };
        Self { active }
    }

    /// Identifies this scope instance (an SCC, a module) in the emitted record.
    /// The closure only runs when timings are enabled.
    pub fn label(&mut self, label: impl FnOnce() -> String) {
        if let Some(active) = &mut self.active {
            active.label = Some(label());
        }
    }

    pub fn mark(&mut self, name: &'static str) {
        if let Some(active) = &mut self.active {
            let now = Instant::now();
            let live = crate::live_bytes::live();
            active.phases.push((
                name,
                (now - active.checkpoint).as_secs_f64() * 1000.0,
                live - active.live,
            ));
            active.checkpoint = now;
            active.live = live;
        }
    }
}

impl Drop for PhaseTimings {
    fn drop(&mut self) {
        self.mark("teardown_or_unfinished");
        let Some(active) = self.active.take() else {
            return;
        };
        let record = serde_json::json!({
            "scope": active.scope,
            "label": active.label,
            "build_mode": if cfg!(debug_assertions) { "debug" } else { "release" },
            "total_ms": active.checkpoint.duration_since(active.started).as_secs_f64() * 1000.0,
            "phases": active.phases.iter().map(|(name, ms, live)| serde_json::json!({"name": name, "ms": ms, "live_delta": live})).collect::<Vec<_>>(),
        });
        let _ = writeln!(std::io::stderr().lock(), "RUST_DIALYZER_TIMING {record}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_checkpoint_does_not_allocate() {
        let mut timings = PhaseTimings { active: None };
        timings.mark("phase");
        assert!(timings.active.is_none());
    }

    #[test]
    fn enabled_checkpoints_partition_elapsed_time() {
        let started = Instant::now();
        let mut timings = PhaseTimings {
            active: Some(Active {
                label: None,
                scope: "test",
                started,
                checkpoint: started,
                live: 0,
                phases: Vec::new(),
            }),
        };
        timings.mark("first");
        timings.mark("second");
        let active = timings.active.take().unwrap();
        assert_eq!(active.phases.len(), 2);
        let sum: f64 = active.phases.iter().map(|(_, ms, _)| ms).sum();
        assert!(
            (sum - active
                .checkpoint
                .duration_since(active.started)
                .as_secs_f64()
                * 1000.0)
                .abs()
                < 0.000001
        );
    }
}
