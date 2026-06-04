//! Sample statistics. No outlier rejection, no confidence intervals —
//! mac single-run noise is ±20-40%, which would swamp any t-statistic
//! we computed. Report min (best-of-N), median (robust center), mean
//! (cheap-stat compat), stddev (rough spread).

pub fn min(samples: &[f64]) -> f64 {
    samples.iter().cloned().fold(f64::INFINITY, f64::min)
}

pub fn mean(samples: &[f64]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    samples.iter().sum::<f64>() / samples.len() as f64
}

pub fn median(samples: &[f64]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
    let n = sorted.len();
    if n % 2 == 0 {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    } else {
        sorted[n / 2]
    }
}

pub fn stddev(samples: &[f64]) -> f64 {
    if samples.len() < 2 {
        return 0.0;
    }
    let m = mean(samples);
    let var = samples
        .iter()
        .map(|x| {
            let d = x - m;
            d * d
        })
        .sum::<f64>()
        / (samples.len() - 1) as f64;
    var.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_samples_all_zero() {
        assert_eq!(mean(&[]), 0.0);
        assert_eq!(median(&[]), 0.0);
        assert_eq!(stddev(&[]), 0.0);
        assert!(min(&[]).is_infinite());
    }

    #[test]
    fn single_sample() {
        let s = [5.0];
        assert_eq!(mean(&s), 5.0);
        assert_eq!(median(&s), 5.0);
        assert_eq!(stddev(&s), 0.0);
        assert_eq!(min(&s), 5.0);
    }

    #[test]
    fn odd_count_uses_middle() {
        let s = [1.0, 2.0, 3.0];
        assert_eq!(median(&s), 2.0);
        assert!((mean(&s) - 2.0).abs() < 1e-9);
        assert_eq!(min(&s), 1.0);
    }

    #[test]
    fn even_count_averages_middle_pair() {
        let s = [1.0, 2.0, 3.0, 4.0];
        assert_eq!(median(&s), 2.5);
        assert!((mean(&s) - 2.5).abs() < 1e-9);
    }

    #[test]
    fn unsorted_input_handled() {
        let s = [3.0, 1.0, 4.0, 1.0, 5.0, 9.0, 2.0, 6.0];
        // sorted: 1, 1, 2, 3, 4, 5, 6, 9 → median = (3 + 4) / 2 = 3.5
        assert_eq!(median(&s), 3.5);
        assert_eq!(min(&s), 1.0);
    }

    #[test]
    fn stddev_one_through_ten() {
        let s: Vec<f64> = (1..=10).map(|x| x as f64).collect();
        // mean = 5.5
        // sample variance = Σ(x - 5.5)² / (n - 1) = 82.5 / 9 ≈ 9.166666...
        // stddev = sqrt(9.1666...) ≈ 3.02765
        let sd = stddev(&s);
        assert!((sd - 3.02765).abs() < 0.001, "stddev was {sd}");
    }
}
