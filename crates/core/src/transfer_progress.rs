use std::time::Duration;

const WARMUP: Duration = Duration::from_millis(500);
const STALE_GAP: Duration = Duration::from_secs(5);
// A time-dependent alpha keeps the smoothing comparable across uneven callbacks.
const SMOOTHING_SECONDS: f64 = 2.0;

#[derive(Default)]
pub(crate) struct TransferEstimator {
    first: Option<Sample>,
    last: Option<Sample>,
    last_output: Option<Sample>,
    smoothed_rate: Option<f64>,
    stalled: bool,
}

#[derive(Clone, Copy)]
struct Sample {
    elapsed: Duration,
    completed: u64,
    total: u64,
}

#[derive(Debug, PartialEq)]
pub(crate) struct Estimate {
    pub bytes_per_second: Option<f64>,
    pub remaining_seconds: Option<f64>,
}

impl TransferEstimator {
    fn reset(&mut self, sample: Sample) -> Estimate {
        self.first = Some(sample);
        self.last = Some(sample);
        self.last_output = Some(sample);
        self.smoothed_rate = None;
        self.stalled = false;
        Estimate {
            bytes_per_second: None,
            remaining_seconds: None,
        }
    }

    /// `elapsed` is measured by the owning operation's monotonic clock. It is
    /// supplied here so tests can exercise time boundaries without sleeping.
    pub fn observe(&mut self, elapsed: Duration, completed: u64, total: u64) -> Estimate {
        let sample = Sample {
            elapsed,
            completed,
            total,
        };
        let Some(last) = self.last else {
            return self.reset(sample);
        };
        let Some(delta) = elapsed.checked_sub(last.elapsed) else {
            return self.reset(sample);
        };
        let last_output = self
            .last_output
            .expect("an initialized estimator has an output baseline");
        if delta >= STALE_GAP || completed < last_output.completed || total != last.total {
            return self.reset(sample);
        }
        if completed > last_output.completed {
            if self.stalled || elapsed.saturating_sub(last_output.elapsed) >= STALE_GAP {
                // The first resumed output is a new baseline. Do not average
                // its burst over the stalled period or revive an old estimate.
                return self.reset(sample);
            }
            self.last_output = Some(sample);
        } else if elapsed.saturating_sub(last_output.elapsed) >= STALE_GAP {
            // Progress callbacks can keep arriving while a solid archive is
            // decoding without writing. Their timestamps are not byte progress.
            // Keep last_output unchanged so identical counters cannot renew it.
            self.stalled = true;
            self.last = Some(sample);
            self.smoothed_rate = None;
            return Estimate {
                bytes_per_second: None,
                remaining_seconds: None,
            };
        }
        if !delta.is_zero() {
            let seconds = delta.as_secs_f64();
            let rate = (completed - last.completed) as f64 / seconds;
            let alpha = -(-seconds / SMOOTHING_SECONDS).exp_m1();
            self.smoothed_rate = Some(match self.smoothed_rate {
                Some(previous) => previous + alpha * (rate - previous),
                None => rate,
            });
            self.last = Some(sample);
        }
        // Equal-time samples do not consume their byte delta. The next timed
        // sample measures it, avoiding a fake rate or losing those bytes.
        let first = self
            .first
            .expect("an initialized estimator has a first sample");
        let warmed_up =
            elapsed.saturating_sub(first.elapsed) >= WARMUP && completed > first.completed;
        let rate = self
            .smoothed_rate
            .filter(|r| warmed_up && r.is_finite() && *r > 0.0);
        let remaining = rate
            .filter(|_| total > 0)
            .map(|r| total.saturating_sub(completed) as f64 / r)
            .filter(|seconds| seconds.is_finite() && *seconds >= 0.0);
        Estimate {
            bytes_per_second: rate,
            remaining_seconds: remaining,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    #[test]
    fn warms_up_and_reports_only_observed_output_rate() {
        let mut tracker = TransferEstimator::default();
        assert_eq!(tracker.observe(at(0), 0, 1000).bytes_per_second, None);
        assert_eq!(tracker.observe(at(499), 499, 1000).bytes_per_second, None);
        let estimate = tracker.observe(at(500), 500, 1000);
        assert!((estimate.bytes_per_second.unwrap() - 1000.0).abs() < 1e-9);
        assert!((estimate.remaining_seconds.unwrap() - 0.5).abs() < 1e-9);
        assert_eq!(
            tracker.observe(at(1000), 1000, 1000).remaining_seconds,
            Some(0.0)
        );
    }

    #[test]
    fn unknown_total_has_speed_without_eta_and_zero_output_has_neither() {
        let mut tracker = TransferEstimator::default();
        tracker.observe(at(0), 0, 0);
        assert_eq!(
            tracker.observe(at(500), 0, 0),
            Estimate {
                bytes_per_second: None,
                remaining_seconds: None
            }
        );
        let moving = tracker.observe(at(1000), 1000, 0);
        assert!(moving.bytes_per_second.is_some());
        assert_eq!(moving.remaining_seconds, None);
    }

    #[test]
    fn long_gap_rollback_total_change_and_backward_time_restart_warmup() {
        for next in [
            (5500, 900, 1000),
            (600, 1, 1000),
            (600, 600, 2000),
            (400, 600, 1000),
        ] {
            let mut tracker = TransferEstimator::default();
            tracker.observe(at(0), 0, 1000);
            assert!(
                tracker
                    .observe(at(500), 500, 1000)
                    .bytes_per_second
                    .is_some()
            );
            assert_eq!(
                tracker.observe(at(next.0), next.1, next.2).bytes_per_second,
                None
            );
            assert_eq!(
                tracker
                    .observe(at(next.0 + 499), next.1 + 100, next.2)
                    .bytes_per_second,
                None
            );
            assert!(
                tracker
                    .observe(at(next.0 + 500), next.1 + 101, next.2)
                    .bytes_per_second
                    .is_some()
            );
        }
    }

    #[test]
    fn equal_time_samples_do_not_drop_bytes_or_create_nonfinite_speed() {
        let mut tracker = TransferEstimator::default();
        tracker.observe(at(0), 0, u64::MAX);
        assert_eq!(tracker.observe(at(0), 100, u64::MAX).bytes_per_second, None);
        let estimate = tracker.observe(at(500), 1000, u64::MAX);
        assert_eq!(estimate.bytes_per_second, Some(2000.0));
        assert!(estimate.remaining_seconds.unwrap().is_finite());
        let large = tracker.observe(at(501), u64::MAX, u64::MAX);
        assert!(large.bytes_per_second.unwrap().is_finite());
        assert_eq!(large.remaining_seconds, Some(0.0));
    }

    #[test]
    fn time_based_smoothing_is_consistent_across_callback_frequencies() {
        let mut frequent = TransferEstimator::default();
        let mut sparse = TransferEstimator::default();
        for tracker in [&mut frequent, &mut sparse] {
            tracker.observe(at(0), 0, 100_000);
            tracker.observe(at(500), 500, 100_000);
        }
        let mut frequent_rate = 0.0;
        for ms in (700..=2500).step_by(200) {
            frequent_rate = frequent
                .observe(at(ms), 500 + (ms - 500) * 2, 100_000)
                .bytes_per_second
                .unwrap();
        }
        let sparse_rate = sparse
            .observe(at(2500), 4500, 100_000)
            .bytes_per_second
            .unwrap();
        assert!((frequent_rate - sparse_rate).abs() < 1e-9);
        assert!(sparse_rate > 1000.0 && sparse_rate < 2000.0);
    }

    #[test]
    fn repeated_counters_expire_speed_and_resumed_output_requires_new_warmup() {
        for interval_ms in [200, 500] {
            let mut tracker = TransferEstimator::default();
            tracker.observe(at(0), 0, 100_000);
            assert_eq!(
                tracker.observe(at(500), 500, 100_000).bytes_per_second,
                Some(1000.0)
            );
            // Frequent callbacks stay within the event-gap threshold, while
            // the last actual output remains at 500 ms.
            for ms in (500 + interval_ms..=6500).step_by(interval_ms as usize) {
                let estimate = tracker.observe(at(ms), 500, 100_000);
                if ms >= 5500 {
                    assert_eq!(
                        estimate,
                        Estimate {
                            bytes_per_second: None,
                            remaining_seconds: None
                        }
                    );
                }
            }
            let resumed = tracker.observe(at(7000), 1000, 100_000);
            assert_eq!(
                resumed,
                Estimate {
                    bytes_per_second: None,
                    remaining_seconds: None
                }
            );
            assert_eq!(
                tracker.observe(at(7499), 1499, 100_000).bytes_per_second,
                None
            );
            let warmed = tracker.observe(at(7500), 1500, 100_000);
            assert_eq!(warmed.bytes_per_second, Some(1000.0));
            assert_eq!(warmed.remaining_seconds, Some(98.5));
        }
    }

    #[test]
    fn equal_time_output_advances_do_not_hide_later_counter_rollback() {
        let mut tracker = TransferEstimator::default();
        tracker.observe(at(0), 0, 1000);
        tracker.observe(at(0), 100, 1000);
        assert_eq!(tracker.observe(at(500), 50, 1000).bytes_per_second, None);
        assert_eq!(tracker.observe(at(999), 549, 1000).bytes_per_second, None);
        assert_eq!(
            tracker.observe(at(1000), 550, 1000).bytes_per_second,
            Some(1000.0)
        );
    }

    #[test]
    fn first_output_after_five_seconds_resets_even_before_a_stalled_callback() {
        let mut tracker = TransferEstimator::default();
        tracker.observe(at(0), 0, 100_000);
        assert!(
            tracker
                .observe(at(500), 500, 100_000)
                .bytes_per_second
                .is_some()
        );
        for ms in (700..=5300).step_by(200) {
            tracker.observe(at(ms), 500, 100_000);
        }
        assert_eq!(
            tracker.observe(at(5500), 1000, 100_000).bytes_per_second,
            None
        );
        assert_eq!(
            tracker.observe(at(5999), 1499, 100_000).bytes_per_second,
            None
        );
        assert_eq!(
            tracker.observe(at(6000), 1500, 100_000).bytes_per_second,
            Some(1000.0)
        );
    }
}
