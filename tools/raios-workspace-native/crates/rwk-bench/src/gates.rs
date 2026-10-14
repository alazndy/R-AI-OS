//! The performance gates of the terminal engine and their evaluation.
//!
//! The limits come from section 9 of the design spec, which calls them hypotheses. Where the
//! spec's number is a raw parse speed, the gate measures exactly that (bytes fed from memory
//! through a pipe, no PTY, no producer process). Where the spec's number is a UI frame rate,
//! which cannot be measured without a UI, the gate is a proxy built from one stated rule: the
//! snapshot the UI takes each frame may use a quarter of the frame budget at the 99th
//! percentile.

/// One gate: a measurement that must not exceed its limit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gate {
    /// Short identifier used in reports.
    pub id: &'static str,
    /// What is measured.
    pub name: &'static str,
    /// Unit of the measurement and the limit.
    pub unit: &'static str,
    /// The measurement must be at most this.
    pub limit: f64,
}

/// 1M plain lines parsed from memory (spec target 0.7 s; spike reference 0.55 s).
pub const PARSE_PLAIN: Gate = Gate {
    id: "G1",
    name: "parse 1M plain lines (from memory)",
    unit: "s",
    limit: 0.70,
};

/// 300K colored lines parsed from memory (spec target 1.0 s; spike reference 0.9 s).
pub const PARSE_COLOR: Gate = Gate {
    id: "G2",
    name: "parse 300K colored lines (from memory)",
    unit: "s",
    limit: 1.00,
};

/// A 1M-line flood through a real PTY takes at most 1.2 times as long as the same flood read
/// by a reader that does nothing: the engine must not be what slows a flood down. The floor
/// is measured in the same run, so a slow or busy machine moves both numbers together.
pub const PTY_OVERHEAD: Gate = Gate {
    id: "G3",
    name: "1M-line PTY flood versus the do-nothing floor",
    unit: "x",
    limit: 1.20,
};

/// Peak resident memory of the process during a 4M-line flood (spec target 256 MB).
pub const FLOOD_RSS: Gate = Gate {
    id: "G4",
    name: "peak RSS during a 4M-line flood",
    unit: "MiB",
    limit: 256.0,
};

/// Proxy for "at least 45 fps during a flood": a frame at 45 fps lasts 22 ms, a quarter of it
/// is 5.5 ms.
pub const FLOOD_SNAPSHOT_P99: Gate = Gate {
    id: "G5",
    name: "snapshot latency p99 during a 4M-line flood",
    unit: "ms",
    limit: 5.5,
};

/// No single snapshot may cost two dropped 60 Hz frames.
pub const FLOOD_SNAPSHOT_MAX: Gate = Gate {
    id: "G6",
    name: "snapshot latency max during a 4M-line flood",
    unit: "ms",
    limit: 33.0,
};

/// Four floods at once drain in bounded time (derived: four times the single-flood limit,
/// since they share the cores).
pub const CONCURRENT_DRAIN: Gate = Gate {
    id: "G7",
    name: "4 concurrent 1M-line floods fully parsed",
    unit: "s",
    limit: 2.80,
};

/// Proxy for "at least 30 fps with four concurrent floods": a frame at 30 fps lasts 33 ms, a
/// quarter of it is 8.3 ms.
pub const CONCURRENT_SNAPSHOT_P99: Gate = Gate {
    id: "G8",
    name: "snapshot latency p99 with 4 concurrent floods",
    unit: "ms",
    limit: 8.3,
};

/// Every headless gate, in report order.
pub const ALL_GATES: [Gate; 8] = [
    PARSE_PLAIN,
    PARSE_COLOR,
    PTY_OVERHEAD,
    FLOOD_RSS,
    FLOOD_SNAPSHOT_P99,
    FLOOD_SNAPSHOT_MAX,
    CONCURRENT_DRAIN,
    CONCURRENT_SNAPSHOT_P99,
];

/// A gate together with what was measured.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outcome {
    /// The gate.
    pub gate: Gate,
    /// The measurement, in the gate's unit.
    pub measured: f64,
}

impl Outcome {
    /// True when the measurement is within the limit.
    pub fn passed(&self) -> bool {
        self.measured <= self.gate.limit
    }

    /// One report line.
    pub fn line(&self) -> String {
        format!(
            "{} {:<50} measured {:>8.3} {:<3} limit {:>7.3} {:<3} {}",
            self.gate.id,
            self.gate.name,
            self.measured,
            self.gate.unit,
            self.gate.limit,
            self.gate.unit,
            if self.passed() { "PASS" } else { "FAIL" }
        )
    }
}

/// True when every outcome passed.
pub fn all_passed(outcomes: &[Outcome]) -> bool {
    outcomes.iter().all(Outcome::passed)
}

/// What a whole run means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Every gate passed.
    Passed,
    /// A gate failed on a quiet machine.
    Failed,
    /// A gate failed, but the machine was busy, so the failure proves nothing.
    Inconclusive,
}

/// Combines the outcomes with the machine state. A pass counts even on a busy machine, because
/// load can only make the numbers worse; a failure counts only when the machine was quiet.
pub fn verdict(outcomes: &[Outcome], machine_was_quiet: bool) -> Verdict {
    match (all_passed(outcomes), machine_was_quiet) {
        (true, _) => Verdict::Passed,
        (false, true) => Verdict::Failed,
        (false, false) => Verdict::Inconclusive,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_measurement_at_the_limit_passes_and_above_it_fails() {
        let at = Outcome {
            gate: PARSE_PLAIN,
            measured: 0.70,
        };
        let over = Outcome {
            gate: PARSE_PLAIN,
            measured: 0.7001,
        };
        assert!(at.passed());
        assert!(!over.passed());
        assert!(!all_passed(&[at, over]));
        assert!(all_passed(&[at]));
    }

    #[test]
    fn gate_identifiers_are_unique_and_limits_positive() {
        let mut ids: Vec<&str> = ALL_GATES.iter().map(|gate| gate.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), ALL_GATES.len());
        assert!(ALL_GATES.iter().all(|gate| gate.limit > 0.0));
    }

    #[test]
    fn a_report_line_names_the_verdict() {
        let line = Outcome {
            gate: FLOOD_RSS,
            measured: 300.0,
        }
        .line();
        assert!(line.starts_with("G4 "), "{line}");
        assert!(line.ends_with("FAIL"), "{line}");
    }

    #[test]
    fn a_failure_on_a_busy_machine_is_inconclusive_but_a_pass_always_counts() {
        let good = Outcome {
            gate: PARSE_PLAIN,
            measured: 0.1,
        };
        let bad = Outcome {
            gate: PARSE_PLAIN,
            measured: 9.0,
        };
        assert_eq!(verdict(&[good], false), Verdict::Passed);
        assert_eq!(verdict(&[good], true), Verdict::Passed);
        assert_eq!(verdict(&[good, bad], true), Verdict::Failed);
        assert_eq!(verdict(&[good, bad], false), Verdict::Inconclusive);
    }
}
