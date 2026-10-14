//! Detects work done by other processes while a measurement runs.
//!
//! The load average cannot be used for this: it counts the benchmark's own threads. Instead the
//! meter compares the whole machine's busy time with the busy time of this process and of the
//! children it has waited for. What remains was done by somebody else.

/// CPU time in clock ticks: `(busy, total)` for the whole machine.
type SystemTicks = (u64, u64);

/// Parses the first line of `/proc/stat` (`cpu user nice system idle iowait irq softirq steal ...`).
pub fn parse_system_ticks(line: &str) -> Option<SystemTicks> {
    let mut fields = line.split_whitespace();
    if fields.next()? != "cpu" {
        return None;
    }
    let values: Vec<u64> = fields.map_while(|field| field.parse().ok()).collect();
    if values.len() < 8 {
        return None;
    }
    let (user, nice, system, idle, iowait, irq, softirq, steal) = (
        values[0], values[1], values[2], values[3], values[4], values[5], values[6], values[7],
    );
    let busy = user + nice + system + irq + softirq + steal;
    Some((busy, busy + idle + iowait))
}

/// Parses `/proc/<pid>/stat` into the ticks this process and its waited-for children used
/// (`utime + stime + cutime + cstime`). The command name may contain spaces and parentheses, so
/// the fields are counted from the last closing parenthesis.
pub fn parse_process_ticks(text: &str) -> Option<u64> {
    let after_name = text.rsplit_once(')')?.1;
    let fields: Vec<&str> = after_name.split_whitespace().collect();
    // After the name: state(0) ppid(1) ... utime is field 14 overall, so index 11 here.
    let take = |index: usize| fields.get(index)?.parse::<u64>().ok();
    Some(take(11)? + take(12)? + take(13)? + take(14)?)
}

fn read_system() -> Option<SystemTicks> {
    let text = std::fs::read_to_string("/proc/stat").ok()?;
    parse_system_ticks(text.lines().next()?)
}

fn read_ours() -> Option<u64> {
    parse_process_ticks(&std::fs::read_to_string("/proc/self/stat").ok()?)
}

/// Measures how much of the machine others used since it was started.
pub struct Meter {
    start_system: SystemTicks,
    start_ours: u64,
}

impl Meter {
    /// Starts a measurement window.
    pub fn start() -> Self {
        Self {
            start_system: read_system().unwrap_or((0, 0)),
            start_ours: read_ours().unwrap_or(0),
        }
    }

    /// The share of the machine's total CPU capacity (0 to 1) that other processes used since
    /// the window started. Zero when the counters cannot be read.
    pub fn foreign_share(&self) -> f64 {
        let (Some((busy, total)), Some(ours)) = (read_system(), read_ours()) else {
            return 0.0;
        };
        foreign_share(
            busy.saturating_sub(self.start_system.0),
            total.saturating_sub(self.start_system.1),
            ours.saturating_sub(self.start_ours),
        )
    }
}

/// Pure form of [`Meter::foreign_share`]: busy and total ticks of the machine and ticks of this
/// process over the same window.
pub fn foreign_share(system_busy: u64, system_total: u64, ours: u64) -> f64 {
    if system_total == 0 {
        return 0.0;
    }
    (system_busy.saturating_sub(ours) as f64 / system_total as f64).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_machine_line_of_proc_stat_is_parsed_into_busy_and_total() {
        let line = "cpu  100 10 50 800 40 5 5 2 0 0";
        // busy = 100+10+50+5+5+2 = 172, total = busy + idle 800 + iowait 40
        assert_eq!(parse_system_ticks(line), Some((172, 1012)));
    }

    #[test]
    fn a_line_that_is_not_the_cpu_summary_or_is_short_is_rejected() {
        assert_eq!(parse_system_ticks("cpu0 1 2 3 4 5 6 7 8"), None);
        assert_eq!(parse_system_ticks("cpu 1 2 3"), None);
        assert_eq!(parse_system_ticks(""), None);
    }

    #[test]
    fn process_ticks_survive_a_command_name_with_spaces_and_parentheses() {
        let text =
            "123 (my (odd) name) S 1 123 123 0 -1 4194560 100 0 0 0 7 3 2 1 20 0 1 0 5 1000 100";
        assert_eq!(parse_process_ticks(text), Some(7 + 3 + 2 + 1));
        assert_eq!(parse_process_ticks("garbage"), None);
    }

    #[test]
    fn only_work_beyond_our_own_counts_as_foreign() {
        assert_eq!(foreign_share(500, 1000, 500), 0.0);
        assert!((foreign_share(500, 1000, 250) - 0.25).abs() < 1e-9);
        assert_eq!(foreign_share(100, 1000, 900), 0.0, "never negative");
        assert_eq!(foreign_share(5, 0, 0), 0.0, "an empty window is not noise");
        assert_eq!(
            foreign_share(2000, 1000, 0),
            1.0,
            "clamped to the whole machine"
        );
    }

    #[test]
    fn the_live_meter_reads_the_counters_of_this_machine() {
        let meter = Meter::start();
        let share = meter.foreign_share();
        assert!((0.0..=1.0).contains(&share));
    }
}
