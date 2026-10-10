//! `ui-bench <scenario|all> [--enforce] [--runs N] [--max-foreign F]`
//!
//! Measures the UI-bound gates of the terminal view (U1-U5) in real GPUI windows. Each
//! measurement runs in its own child process, because a GPUI application runs once per process.
//!
//! Scenarios: `cold-start` (U1), `flood` (U2), `flood4` (U3), `keystroke` (U4), `idle` (U5),
//! `all`. Rates and times use the median of the runs, latencies and memory the worst.
//!
//! Exit status with `--enforce`: 0 every gate passed, 1 a gate failed on a quiet machine,
//! 3 inconclusive (a failure on a busy machine, or a gate that could not be measured because the
//! compositor delivered no frames to the window), 2 usage or setup error.
//!
//! "Quiet" means other processes used at most `--max-foreign` of the machine's CPU capacity
//! (default 25%) before and during the run. Frames are painted frames, not presented ones, and
//! keystroke latency excludes compositor and display; the report says so.

mod child;
mod gates;
mod machine;
mod stats;

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use child::Scenario;
use gates::{
    parse_result_line, verdict, Gate, Outcome, Verdict, COLD_START, FLOOD4_FPS, FLOOD_FPS,
    IDLE_PSS, KEYSTROKE_P99,
};
use machine::Meter;
use stats::median;

const QUIET_WAIT: Duration = Duration::from_secs(90);
const CHILD_DEADLINE: Duration = Duration::from_secs(240);

struct Options {
    scenario: String,
    enforce: bool,
    runs: usize,
    max_foreign: f64,
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        scenario: String::new(),
        enforce: false,
        runs: 3,
        max_foreign: 0.25,
    };
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--enforce" => options.enforce = true,
            "--runs" => {
                options.runs = iter
                    .next()
                    .and_then(|value| value.parse().ok())
                    .filter(|runs| (1..=50).contains(runs))
                    .ok_or("--runs needs a number from 1 to 50")?;
            }
            "--max-foreign" => {
                options.max_foreign = iter
                    .next()
                    .and_then(|value| value.parse().ok())
                    .filter(|share| *share > 0.0 && *share <= 1.0)
                    .ok_or("--max-foreign needs a number above 0 and at most 1")?;
            }
            name if options.scenario.is_empty() && !name.starts_with("--") => {
                options.scenario = name.to_owned();
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    if options.scenario.is_empty() {
        return Err("name a scenario: cold-start, flood, flood4, keystroke, idle or all".into());
    }
    Ok(options)
}

/// One child run's measurement, or why there is none.
enum Run {
    Value(f64),
    NotMeasured(String),
}

fn child_command(scenario: &str) -> Result<Command, String> {
    let mut command = Command::new(child::current_exe()?);
    command.args(["--child", scenario]);
    Ok(command)
}

/// Spawn to the child's `FIRST_FRAME` line.
fn run_cold_start() -> Result<Run, String> {
    let mut command = child_command("cold-start")?;
    command.stdout(Stdio::piped());
    let started = Instant::now();
    let mut process = command.spawn().map_err(|error| error.to_string())?;
    let stdout = process.stdout.take().ok_or("no child stdout")?;
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    let result = loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break Run::NotMeasured("the child exited before painting".into()),
            Ok(_) if line.trim() == "FIRST_FRAME" => {
                break Run::Value(started.elapsed().as_secs_f64() * 1000.0)
            }
            Ok(_) => {}
        }
        if started.elapsed() > CHILD_DEADLINE {
            break Run::NotMeasured("no frame within the deadline".into());
        }
    };
    let _ = process.kill();
    let _ = process.wait();
    Ok(result)
}

/// Runs a child that prints one `RESULT` line and returns the value of `key`.
fn run_result(scenario: &str, key: &str) -> Result<Run, String> {
    let mut command = child_command(scenario)?;
    command.stdout(Stdio::piped());
    let output = command.output().map_err(|error| error.to_string())?;
    let text = String::from_utf8_lossy(&output.stdout);
    let Some(pairs) = text.lines().find_map(parse_result_line) else {
        return Ok(Run::NotMeasured(format!(
            "the child printed no result (exit {:?})",
            output.status.code()
        )));
    };
    let get = |wanted: &str| {
        pairs
            .iter()
            .find(|(k, _)| k == wanted)
            .map(|(_, v)| v.as_str())
    };
    match get("status") {
        Some("ok") => get(key)
            .and_then(|value| value.parse().ok())
            .map(Run::Value)
            .ok_or_else(|| format!("the result has no numeric {key}")),
        Some(other) => Ok(Run::NotMeasured(format!(
            "{scenario}: {other} (the compositor delivered no frames to the window)"
        ))),
        None => Err("the result has no status".into()),
    }
}

fn repeat(runs: usize, mut one: impl FnMut() -> Result<Run, String>) -> Result<Vec<Run>, String> {
    (0..runs).map(|_| one()).collect()
}

/// Folds the runs of one gate into an outcome, or the reason it could not be measured.
fn outcome(gate: Gate, runs: Vec<Run>, worst: bool) -> Result<Outcome, String> {
    let mut values = Vec::new();
    for run in runs {
        match run {
            Run::Value(value) => values.push(value),
            Run::NotMeasured(reason) => return Err(reason),
        }
    }
    let measured = if worst {
        match gate.direction {
            gates::Direction::AtMost => values.iter().copied().fold(f64::MIN, f64::max),
            gates::Direction::AtLeast => values.iter().copied().fold(f64::MAX, f64::min),
        }
    } else {
        median(&values)
    };
    Ok(Outcome { gate, measured })
}

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

/// A gate's outcome, or the gate with the reason it could not be measured.
type GateResult = Result<Outcome, (Gate, String)>;

fn measure(name: &str, runs: usize) -> Result<Vec<GateResult>, String> {
    let wanted: Vec<&str> = if name == "all" {
        vec!["cold-start", "flood", "flood4", "keystroke", "idle"]
    } else {
        vec![name]
    };
    let mut results = Vec::new();
    for scenario in wanted {
        let (gate, worst, runs_done) = match scenario {
            "cold-start" => (COLD_START, false, repeat(runs, run_cold_start)?),
            "flood" => (
                FLOOD_FPS,
                false,
                repeat(runs, || run_result("flood", "fps"))?,
            ),
            "flood4" => (
                FLOOD4_FPS,
                false,
                repeat(runs, || run_result("flood4", "fps"))?,
            ),
            "keystroke" => (
                KEYSTROKE_P99,
                true,
                repeat(runs, || run_result("keystroke", "p99_ms"))?,
            ),
            "idle" => (
                IDLE_PSS,
                true,
                repeat(runs, || run_result("idle", "pss_mb"))?,
            ),
            other => return Err(format!("unknown scenario {other:?}")),
        };
        results.push(outcome(gate, runs_done, worst).map_err(|reason| (gate, reason)));
    }
    Ok(results)
}

fn real_main(args: &[String]) -> i32 {
    if args.first().map(String::as_str) == Some("--child") {
        let Some(scenario) = args.get(1).and_then(|name| Scenario::parse(name)) else {
            eprintln!("--child needs a scenario name");
            return 2;
        };
        child::run(scenario);
    }
    let options = match parse_args(args) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("ui-bench: {message}");
            return 2;
        }
    };
    let before = wait_for_quiet(options.max_foreign);
    println!(
        "foreign cpu: {:.0}% of the machine before the run (quiet means at most {:.0}%); {} runs per gate, median for rates and times, worst for latencies and memory\n",
        before * 100.0,
        options.max_foreign * 100.0,
        options.runs
    );
    let meter = Meter::start();
    let results = match measure(&options.scenario, options.runs) {
        Ok(results) => results,
        Err(message) => {
            eprintln!("ui-bench: {message}");
            return 2;
        }
    };
    let during = meter.foreign_share();
    let mut outcomes = Vec::new();
    let mut unmeasured = 0;
    for result in results {
        match result {
            Ok(outcome) => {
                println!("{}", outcome.line());
                outcomes.push(outcome);
            }
            Err((gate, reason)) => {
                unmeasured += 1;
                println!("{} {:<46} NOT MEASURED: {reason}", gate.id, gate.name);
            }
        }
    }
    println!("\nforeign cpu during the run: {:.0}%", during * 100.0);
    println!(
        "frames are painted frames, not presented ones; keystroke latency excludes compositor and display"
    );
    let quiet = before <= options.max_foreign && during <= options.max_foreign;
    let (text, code) = match verdict(&outcomes, unmeasured, quiet) {
        Verdict::Passed => ("ALL GATES PASSED", 0),
        Verdict::Failed => ("AT LEAST ONE GATE FAILED ON A QUIET MACHINE", 1),
        Verdict::Inconclusive => (
            "INCONCLUSIVE: a gate failed on a busy machine or could not be measured; run again when it is quiet and the window receives frames",
            3,
        ),
    };
    println!("{text}");
    if options.enforce {
        code
    } else {
        0
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(real_main(&args));
}
