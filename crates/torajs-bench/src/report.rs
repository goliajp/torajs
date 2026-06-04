use crate::stats::{mean, median, min, stddev};

fn fmt_duration_ns(ns: f64) -> String {
    if ns < 1_000.0 {
        format!("{ns:.2} ns")
    } else if ns < 1_000_000.0 {
        format!("{:.2} µs", ns / 1_000.0)
    } else if ns < 1_000_000_000.0 {
        format!("{:.2} ms", ns / 1_000_000.0)
    } else {
        format!("{:.2} s", ns / 1_000_000_000.0)
    }
}

pub fn report(name: &str, iters: u64, samples: &[f64]) {
    if samples.is_empty() {
        println!("{name}  [no samples]");
        return;
    }
    let mn = min(samples);
    let md = median(samples);
    let av = mean(samples);
    let sd = stddev(samples);
    println!(
        "{:<44}  n={:<3} iters={:<10}  min={:>10}  median={:>10}  mean={:>10}  stddev={:>10}",
        name,
        samples.len(),
        iters,
        fmt_duration_ns(mn),
        fmt_duration_ns(md),
        fmt_duration_ns(av),
        fmt_duration_ns(sd),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_duration_ns_picks_unit_by_magnitude() {
        assert_eq!(fmt_duration_ns(0.5), "0.50 ns");
        assert_eq!(fmt_duration_ns(999.0), "999.00 ns");
        assert_eq!(fmt_duration_ns(1_500.0), "1.50 µs");
        assert_eq!(fmt_duration_ns(2_500_000.0), "2.50 ms");
        assert_eq!(fmt_duration_ns(3_500_000_000.0), "3.50 s");
    }
}
