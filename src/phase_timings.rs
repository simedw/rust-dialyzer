use std::{io::Write, time::Instant};

/// Opt-in sequential timings. Nested scopes overlap and are reported separately.
pub struct PhaseTimings {
    active: Option<Active>,
}

struct Active {
    scope: &'static str,
    started: Instant,
    checkpoint: Instant,
    phases: Vec<(&'static str, f64)>,
}

impl PhaseTimings {
    pub fn new(scope: &'static str) -> Self {
        let active = if std::env::var_os("RUST_DIALYZER_TIMINGS").as_deref()
            == Some(std::ffi::OsStr::new("1"))
        {
            let started = Instant::now();
            Some(Active {
                scope,
                started,
                checkpoint: started,
                phases: Vec::new(),
            })
        } else {
            None
        };
        Self { active }
    }

    pub fn mark(&mut self, name: &'static str) {
        if let Some(active) = &mut self.active {
            let now = Instant::now();
            active
                .phases
                .push((name, (now - active.checkpoint).as_secs_f64() * 1000.0));
            active.checkpoint = now;
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
            "build_mode": if cfg!(debug_assertions) { "debug" } else { "release" },
            "total_ms": active.checkpoint.duration_since(active.started).as_secs_f64() * 1000.0,
            "phases": active.phases.iter().map(|(name, ms)| serde_json::json!({"name": name, "ms": ms})).collect::<Vec<_>>(),
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
                scope: "test",
                started,
                checkpoint: started,
                phases: Vec::new(),
            }),
        };
        timings.mark("first");
        timings.mark("second");
        let active = timings.active.take().unwrap();
        assert_eq!(active.phases.len(), 2);
        let sum: f64 = active.phases.iter().map(|(_, ms)| ms).sum();
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
