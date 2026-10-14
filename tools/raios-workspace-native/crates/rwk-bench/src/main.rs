//! `term-bench`: measures the headless performance gates of the terminal engine.
//!
//! `term-bench [--enforce] [--runs N] [--max-foreign F]`
//!
//! Prints one line per gate and a verdict. With `--enforce` the exit status is 0 when every gate
//! passed, 1 when a gate failed on a quiet machine, 3 when a gate failed but the machine was
//! busy, so the failure is inconclusive; usage and setup errors are always 2. Without
//! `--enforce` the status is 0 whenever the measurement itself worked, so the tool can be used
//! for exploration. Run it with a release build on the reference machine.
//!
//! "Quiet" means other processes used at most `--max-foreign` of the machine's CPU capacity
//! (default 0.25), before the run and during it. The tool counts only work of other processes:
//! its own threads and children are subtracted. It waits up to 90 seconds at the start for the
//! machine to become quiet.

mod gates;
mod load;
mod scenarios;
mod stats;

use std::process::ExitCode;
use std::time::{Duration, Instant};

use gates::{
    verdict, Outcome, Verdict, ALL_GATES, CONCURRENT_DRAIN, CONCURRENT_SNAPSHOT_P99, FLOOD_RSS,
    FLOOD_SNAPSHOT_MAX, FLOOD_SNAPSHOT_P99, PARSE_COLOR, PARSE_PLAIN, PTY_OVERHEAD,
};
use load::Meter;
use scenarios::{
    cpu_model, payload, peak_rss_mib, pty_floor, run_parse_only, run_pty_flood, thread_count,
    FloodKind, FloodResult,
};
use stats::median;

const FLOOD_LINES: u64 = 4_000_000;
const QUIET_WAIT: Duration = Duration::from_secs(90);

struct Options {
    enforce: bool,
    runs: usize,
    max_foreign: f64,
}

fn parse_options() -> Result<Options, String> {
    let mut options = Options {
        enforce: false,
        runs: 3,
        max_foreign: 0.25,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--enforce" => options.enforce = true,
            "--runs" => {
                options.runs = args
                    .next()
                    .and_then(|value| value.parse().ok())
                    .filter(|runs| (1..=25).contains(runs))
                    .ok_or("--runs needs a number from 1 to 25")?;
            }
            "--max-foreign" => {
                options.max_foreign = args
                    .next()
                    .and_then(|value| value.parse().ok())
                    .filter(|share: &f64| *share > 0.0 && *share <= 1.0)
                    .ok_or("--max-foreign needs a number above 0 and at most 1")?;
            }
            other => {
                return Err(format!(
                    "unknown argument {other:?}; use --enforce, --runs N, --max-foreign F"
                ))
            }
        }
    }
    Ok(options)
}

fn repeat<T>(
    runs: usize,
    mut scenario: impl FnMut() -> Result<T, String>,
) -> Result<Vec<T>, String> {
    (0..runs).map(|_| scenario()).collect()
}

fn seconds(results: &[FloodResult]) -> f64 {
    median(
        &results
            .iter()
            .map(|r| r.elapsed.as_secs_f64())
            .collect::<Vec<_>>(),
    )
}

fn worst_ms(results: &[FloodResult], pick: impl Fn(&FloodResult) -> u64) -> f64 {
    results.iter().map(pick).max().unwrap_or(0) as f64 / 1000.0
}

/// Waits until other processes use at most `max_foreign` of the machine, sampling in windows of
/// two seconds; returns the share of the last window.
fn wait_for_quiet(max_foreign: f64) -> f64 {
    let deadline = Instant::now() + QUIET_WAIT;
    loop {
        let meter = Meter::start();
        std::thread::sleep(Duration::from_secs(2));
        let share = meter.foreign_share();
        if share <= max_foreign || Instant::now() >= deadline {
            return share;
        }
    }
}

fn run(options: &Options) -> Result<Vec<Outcome>, String> {
    let sampler_single = Some(Duration::from_millis(8));
    let sampler_four = Some(Duration::from_millis(16));
    let mut outcomes = Vec::new();

    // The flood runs first: peak RSS is a process-lifetime high-water mark.
    let flood = repeat(options.runs, || {
        run_pty_flood(FloodKind::Plain, FLOOD_LINES, 1, sampler_single)
    })?;
    outcomes.push(Outcome {
        gate: FLOOD_RSS,
        measured: peak_rss_mib(),
    });
    outcomes.push(Outcome {
        gate: FLOOD_SNAPSHOT_P99,
        measured: worst_ms(&flood, |r| r.latency.p99_us),
    });
    outcomes.push(Outcome {
        gate: FLOOD_SNAPSHOT_MAX,
        measured: worst_ms(&flood, |r| r.latency.max_us),
    });
    println!(
        "flood      : {} MB through a PTY in {:.2} s (median), {} snapshots, {} wake-ups",
        flood[0].bytes / 1_000_000,
        seconds(&flood),
        flood[0].latency.count,
        flood[0].wakes
    );

    let plain_bytes = payload(FloodKind::Plain, 1_000_000);
    let color_bytes = payload(FloodKind::Color, 300_000);
    let parse_plain = repeat(options.runs, || run_parse_only(&plain_bytes))?;
    let parse_color = repeat(options.runs, || run_parse_only(&color_bytes))?;
    outcomes.push(Outcome {
        gate: PARSE_PLAIN,
        measured: seconds(&parse_plain),
    });
    outcomes.push(Outcome {
        gate: PARSE_COLOR,
        measured: seconds(&parse_color),
    });

    let floor = median(&repeat(options.runs, || {
        pty_floor(FloodKind::Plain, 1_000_000).map(|d| d.as_secs_f64())
    })?);
    let through_pty = repeat(options.runs, || {
        run_pty_flood(FloodKind::Plain, 1_000_000, 1, None)
    })?;
    outcomes.push(Outcome {
        gate: PTY_OVERHEAD,
        measured: seconds(&through_pty) / floor,
    });
    println!(
        "pty plain  : {:.3} s through the engine, {:.3} s floor (reader discards)",
        seconds(&through_pty),
        floor
    );

    let color_floor = median(&repeat(options.runs, || {
        pty_floor(FloodKind::Color, 300_000).map(|d| d.as_secs_f64())
    })?);
    let color_pty = repeat(options.runs, || {
        run_pty_flood(FloodKind::Color, 300_000, 1, None)
    })?;
    println!(
        "pty colored: {:.3} s through the engine, {:.3} s floor, {:.3} s parse-only (not a gate)",
        seconds(&color_pty),
        color_floor,
        seconds(&parse_color)
    );

    let concurrent = repeat(options.runs, || {
        run_pty_flood(FloodKind::Plain, 1_000_000, 4, sampler_four)
    })?;
    outcomes.push(Outcome {
        gate: CONCURRENT_DRAIN,
        measured: seconds(&concurrent),
    });
    outcomes.push(Outcome {
        gate: CONCURRENT_SNAPSHOT_P99,
        measured: worst_ms(&concurrent, |r| r.latency.p99_us),
    });

    outcomes.sort_by_key(|outcome| outcome.gate.id);
    let reported: Vec<&str> = outcomes.iter().map(|outcome| outcome.gate.id).collect();
    let expected: Vec<&str> = ALL_GATES.iter().map(|gate| gate.id).collect();
    if reported != expected {
        return Err(format!(
            "the report covers {reported:?}, expected {expected:?}"
        ));
    }
    Ok(outcomes)
}

fn main() -> ExitCode {
    let options = match parse_options() {
        Ok(options) => options,
        Err(message) => {
            eprintln!("term-bench: {message}");
            return ExitCode::from(2);
        }
    };
    println!("machine    : {}, {} threads", cpu_model(), thread_count());
    let before = wait_for_quiet(options.max_foreign);
    println!(
        "foreign cpu: {:.0}% of the machine before the run (quiet means at most {:.0}%); {} runs per gate, median for times, worst for latencies\n",
        before * 100.0,
        options.max_foreign * 100.0,
        options.runs
    );
    let meter = Meter::start();
    let outcomes = match run(&options) {
        Ok(outcomes) => outcomes,
        Err(message) => {
            eprintln!("term-bench: {message}");
            return ExitCode::from(2);
        }
    };
    let during = meter.foreign_share();
    println!();
    for outcome in &outcomes {
        println!("{}", outcome.line());
    }
    let quiet = before <= options.max_foreign && during <= options.max_foreign;
    let verdict = verdict(&outcomes, quiet);
    println!("\nforeign cpu during the run: {:.0}%", during * 100.0);
    let (text, code) = match verdict {
        Verdict::Passed => ("ALL HEADLESS GATES PASSED", 0),
        Verdict::Failed => ("AT LEAST ONE GATE FAILED ON A QUIET MACHINE", 1),
        Verdict::Inconclusive => (
            "INCONCLUSIVE: a gate failed, but the machine was busy; run again when it is quiet",
            3,
        ),
    };
    println!("{text}");
    if options.enforce {
        return ExitCode::from(code);
    }
    ExitCode::SUCCESS
}
