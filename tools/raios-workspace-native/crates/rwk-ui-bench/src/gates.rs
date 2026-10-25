//! The UI-bound gates of the terminal view and their evaluation.
//!
//! The limits come from section 9 of the design spec, which calls them hypotheses. Frames are
//! counted when the view paints, not when the compositor presents them (decision D6), and a
//! keystroke is timed from the key event to the paint that shows its echo; compositor and
//! display latency are not included.

/// Which side of the limit passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// The measurement must not exceed the limit.
    AtMost,
    /// The measurement must reach the limit.
    AtLeast,
}

/// One gate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gate {
    /// Short identifier used in reports.
    pub id: &'static str,
    /// What is measured.
    pub name: &'static str,
    /// Unit of the measurement and the limit.
    pub unit: &'static str,
    /// The limit.
    pub limit: f64,
    /// Which side passes.
    pub direction: Direction,
}

/// Process spawn to the first painted frame of one terminal window.
pub const COLD_START: Gate = Gate {
    id: "U1",
    name: "spawn to first painted frame",
    unit: "ms",
    limit: 300.0,
    direction: Direction::AtMost,
};

/// Painted frames per second during a 4M-line flood in one pane.
pub const FLOOD_FPS: Gate = Gate {
    id: "U2",
    name: "painted frames per second, 4M-line flood",
    unit: "fps",
    limit: 45.0,
    direction: Direction::AtLeast,
};

/// Painted frames per second with four panes flooding at once.
pub const FLOOD4_FPS: Gate = Gate {
    id: "U3",
    name: "painted frames per second, 4 concurrent floods",
    unit: "fps",
    limit: 30.0,
    direction: Direction::AtLeast,
};

/// Key event to the paint that shows the echo, 99th percentile.
pub const KEYSTROKE_P99: Gate = Gate {
    id: "U4",
    name: "keystroke to painted echo, p99",
    unit: "ms",
    limit: 16.0,
    direction: Direction::AtMost,
};

/// Proportional set size of the process with four idle terminals.
pub const IDLE_PSS: Gate = Gate {
    id: "U5",
    name: "idle PSS with 4 live terminals",
    unit: "MB",
    limit: 150.0,
    direction: Direction::AtMost,
};

/// Editor open of a 30K-line file, first painted frame after `open` (spec section 9).
pub const EDITOR_OPEN_30K: Gate = Gate {
    id: "E1",
    name: "editor open, 30K lines, to first painted frame",
    unit: "ms",
    limit: 350.0,
    direction: Direction::AtMost,
};

/// Editor open of a 500K-line buffer. A capability gate of the component: 500K lines exceed
/// the product's 2 MiB document limit, so no product file is this large.
pub const EDITOR_OPEN_500K: Gate = Gate {
    id: "E2",
    name: "editor open, 500K lines (capability), to first frame",
    unit: "ms",
    limit: 700.0,
    direction: Direction::AtMost,
};

/// `rwk-app` spawn to its first painted frame. The 300 ms hypothesis of the spec was raised to
/// 350 ms by the owner on 2026-10-14 after native Wayland measured 306-352 ms, of which
/// 230-290 ms is GPUI's own platform bring-up (see the gate report).
pub const APP_COLD_START: Gate = Gate {
    id: "A1",
    name: "rwk-app spawn to first frame",
    unit: "ms",
    limit: 350.0,
    direction: Direction::AtMost,
};

/// Proportional set size with a project, one open file and four live terminals.
pub const APP_IDLE_PSS: Gate = Gate {
    id: "A2",
    name: "rwk-app idle PSS, project + file + 4 terminals",
    unit: "MB",
    limit: 150.0,
    direction: Direction::AtMost,
};

/// What a dock arrangement command adds to the frame pacing of the window, with twelve live
/// terminals: its 99th-percentile time to the next painted frame minus the median time a bare
/// refresh takes (about 24 ms on the development compositor, which is pacing, not work). The
/// limit, 25 ms, is a hypothesis of about one and a half 60 Hz frames.
pub const DOCK_ARRANGE_OVER_FLOOR: Gate = Gate {
    id: "D1",
    name: "dock arrange over frame floor, 12 terminals, p99",
    unit: "ms",
    limit: 25.0,
    direction: Direction::AtMost,
};

/// How many PTY resizes one dock arrangement command causes, with twelve live terminals. The
/// count is deterministic (258 resizes over 160 commands, 1.6 each, on every run so far), so
/// this catches a layout that makes terminals resize over and over: a storm would be near
/// twelve per command. The limit, 3, is a hypothesis of about twice what is measured.
pub const DOCK_RESIZES_PER_OP: Gate = Gate {
    id: "D2",
    name: "dock arrange PTY resizes per command",
    unit: "n",
    limit: 3.0,
    direction: Direction::AtMost,
};

/// Every gate, in report order.
#[cfg(test)]
pub const ALL_GATES: [Gate; 11] = [
    COLD_START,
    FLOOD_FPS,
    FLOOD4_FPS,
    KEYSTROKE_P99,
    IDLE_PSS,
    EDITOR_OPEN_30K,
    EDITOR_OPEN_500K,
    APP_COLD_START,
    APP_IDLE_PSS,
    DOCK_ARRANGE_OVER_FLOOR,
    DOCK_RESIZES_PER_OP,
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
    /// True when the measurement is on the passing side of the limit.
    pub fn passed(&self) -> bool {
        match self.gate.direction {
            Direction::AtMost => self.measured <= self.gate.limit,
            Direction::AtLeast => self.measured >= self.gate.limit,
        }
    }

    /// One report line.
    pub fn line(&self) -> String {
        let relation = match self.gate.direction {
            Direction::AtMost => "limit <=",
            Direction::AtLeast => "limit >=",
        };
        format!(
            "{} {:<46} measured {:>9.3} {:<3} {} {:>7.3} {:<3} {}",
            self.gate.id,
            self.gate.name,
            self.measured,
            self.gate.unit,
            relation,
            self.gate.limit,
            self.gate.unit,
            if self.passed() { "PASS" } else { "FAIL" }
        )
    }
}

/// What a whole run means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Every gate passed.
    Passed,
    /// A gate failed on a quiet machine.
    Failed,
    /// A gate failed, or could not be measured, so the run proves nothing.
    Inconclusive,
}

/// Combines the outcomes with the machine state. A pass counts even on a busy machine, because
/// load can only make the numbers worse; a failure counts only when the machine was quiet.
/// A gate that could not be measured (`unmeasured`, for example because the compositor
/// delivered no frames) makes the run inconclusive, never passed.
pub fn verdict(outcomes: &[Outcome], unmeasured: usize, machine_was_quiet: bool) -> Verdict {
    let all_passed = outcomes.iter().all(Outcome::passed);
    match (all_passed, unmeasured > 0, machine_was_quiet) {
        (_, true, _) => Verdict::Inconclusive,
        (true, false, _) => Verdict::Passed,
        (false, false, true) => Verdict::Failed,
        (false, false, false) => Verdict::Inconclusive,
    }
}

/// One `key=value` pair of a child's result line.
pub fn parse_result_line(line: &str) -> Option<Vec<(String, String)>> {
    let rest = line.strip_prefix("RESULT ")?;
    rest.split_whitespace()
        .map(|pair| {
            let (key, value) = pair.split_once('=')?;
            Some((key.to_owned(), value.to_owned()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(gate: Gate, measured: f64) -> Outcome {
        Outcome { gate, measured }
    }

    #[test]
    fn at_most_gates_pass_at_the_limit_and_fail_above_it() {
        assert!(outcome(COLD_START, 300.0).passed());
        assert!(!outcome(COLD_START, 300.1).passed());
    }

    #[test]
    fn the_p3_gates_have_the_limits_of_the_plan() {
        assert_eq!(
            (
                EDITOR_OPEN_30K.limit,
                EDITOR_OPEN_500K.limit,
                APP_COLD_START.limit,
                APP_IDLE_PSS.limit
            ),
            (350.0, 700.0, 350.0, 150.0)
        );
        for gate in [
            EDITOR_OPEN_30K,
            EDITOR_OPEN_500K,
            APP_COLD_START,
            APP_IDLE_PSS,
        ] {
            assert_eq!(gate.direction, Direction::AtMost, "{}", gate.id);
        }
        assert!(outcome(EDITOR_OPEN_30K, 351.0).line().ends_with("FAIL"));
    }

    #[test]
    fn at_least_gates_pass_at_the_limit_and_fail_below_it() {
        assert!(outcome(FLOOD_FPS, 45.0).passed());
        assert!(!outcome(FLOOD_FPS, 44.9).passed());
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
    fn a_failure_on_a_busy_machine_is_inconclusive_but_a_pass_always_counts() {
        let failing = [outcome(FLOOD_FPS, 10.0)];
        let passing = [outcome(FLOOD_FPS, 60.0)];
        assert_eq!(verdict(&failing, 0, true), Verdict::Failed);
        assert_eq!(verdict(&failing, 0, false), Verdict::Inconclusive);
        assert_eq!(verdict(&passing, 0, false), Verdict::Passed);
    }

    #[test]
    fn a_gate_that_could_not_be_measured_is_never_a_pass() {
        let passing = [outcome(COLD_START, 100.0)];
        assert_eq!(verdict(&passing, 1, true), Verdict::Inconclusive);
    }

    #[test]
    fn report_lines_state_the_direction_and_the_verdict() {
        let line = outcome(FLOOD_FPS, 30.0).line();
        assert!(line.starts_with("U2 "), "{line}");
        assert!(line.contains("limit >="), "{line}");
        assert!(line.ends_with("FAIL"), "{line}");
    }

    #[test]
    fn a_child_result_line_is_parsed_into_pairs() {
        let pairs = parse_result_line("RESULT fps=61.5 frames=300 slow=2").unwrap();
        assert_eq!(pairs[0], ("fps".to_owned(), "61.5".to_owned()));
        assert_eq!(pairs.len(), 3);
        assert!(parse_result_line("noise").is_none());
        assert!(parse_result_line("RESULT broken").is_none());
    }
}
