//! Latency statistics.

/// Percentiles of a set of latency samples, in microseconds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LatencySummary {
    /// Number of samples.
    pub count: usize,
    /// Median.
    pub p50_us: u64,
    /// 99th percentile.
    pub p99_us: u64,
    /// Slowest sample.
    pub max_us: u64,
}

/// Nearest-rank percentile of an ascending slice; 0 for an empty slice.
pub fn percentile(sorted: &[u64], percent: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = ((percent / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// Summarizes samples; the input order does not matter.
pub fn summarize(mut samples: Vec<u64>) -> LatencySummary {
    samples.sort_unstable();
    LatencySummary {
        count: samples.len(),
        p50_us: percentile(&samples, 50.0),
        p99_us: percentile(&samples, 99.0),
        max_us: samples.last().copied().unwrap_or(0),
    }
}

/// Median of seconds measurements; 0 for an empty slice.
pub fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    sorted[sorted.len() / 2]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_use_the_nearest_rank() {
        let samples: Vec<u64> = (1..=100).collect();
        assert_eq!(percentile(&samples, 50.0), 50);
        assert_eq!(percentile(&samples, 99.0), 99);
        assert_eq!(percentile(&samples, 100.0), 100);
        assert_eq!(percentile(&samples, 0.0), 1);
    }

    #[test]
    fn an_empty_sample_set_summarizes_to_zero() {
        assert_eq!(summarize(Vec::new()), LatencySummary::default());
        assert_eq!(median(&[]), 0.0);
    }

    #[test]
    fn the_summary_ignores_input_order() {
        let summary = summarize(vec![900, 10, 500, 20, 30]);
        assert_eq!(
            summary,
            LatencySummary {
                count: 5,
                p50_us: 30,
                p99_us: 900,
                max_us: 900
            }
        );
    }

    #[test]
    fn the_median_of_three_is_the_middle_value() {
        assert_eq!(median(&[0.9, 0.5, 0.7]), 0.7);
    }
}
