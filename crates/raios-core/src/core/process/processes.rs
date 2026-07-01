use std::process::Command;

use super::ProcessEntry;

pub fn list_processes(top_n: usize) -> Vec<ProcessEntry> {
    if cfg!(target_os = "windows") {
        list_processes_windows(top_n)
    } else {
        list_processes_unix(top_n)
    }
}

fn list_processes_windows(top_n: usize) -> Vec<ProcessEntry> {
    let out = Command::new("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();

    let mut entries: Vec<ProcessEntry> = out
        .lines()
        .filter_map(|line| {
            let cols: Vec<&str> = line.splitn(5, ',').collect();
            if cols.len() < 5 {
                return None;
            }
            let name = cols[0].trim_matches('"').to_string();
            let pid: u32 = cols[1].trim_matches('"').parse().ok()?;
            let mem_kb: f32 = cols[4]
                .trim_matches('"')
                .replace(',', "")
                .replace(" K", "")
                .trim()
                .parse()
                .unwrap_or(0.0);
            Some(ProcessEntry {
                pid,
                name,
                cpu_pct: None,
                mem_mb: Some(mem_kb / 1024.0),
            })
        })
        .collect();

    entries.sort_by(|a, b| {
        b.mem_mb
            .unwrap_or(0.0)
            .partial_cmp(&a.mem_mb.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    entries.truncate(top_n);
    entries
}

fn list_processes_unix(top_n: usize) -> Vec<ProcessEntry> {
    let out = Command::new("ps")
        .args(["-Ao", "pid=,pcpu=,rss=,comm="])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();

    let mut entries: Vec<ProcessEntry> = out
        .lines()
        .filter_map(|line| {
            let mut cols = line.split_whitespace();
            let pid: u32 = cols.next()?.parse().ok()?;
            let cpu: f32 = cols.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
            let rss_kb: f32 = cols.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
            let name = cols.next()?;
            let name = name.split('/').next_back().unwrap_or(name).to_string();
            Some(ProcessEntry {
                pid,
                name,
                cpu_pct: Some(cpu),
                mem_mb: Some(rss_kb / 1024.0),
            })
        })
        .collect();

    entries.sort_by(|a, b| {
        b.mem_mb
            .unwrap_or(0.0)
            .partial_cmp(&a.mem_mb.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    entries.truncate(top_n);
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_processes_bounded() {
        let procs = list_processes(5);
        assert!(procs.len() <= 5);
        for p in &procs {
            assert!(p.pid > 0);
        }
    }
}
