//! Retry policy with exponential backoff and jitter.

use std::time::Duration;

/// Configuration for request retries with exponential backoff and jitter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Maximum number of attempts including the initial request.
    pub max_attempts: u32,
    /// Base delay before the first retry.
    pub base_delay: Duration,
    /// Maximum cap for computed backoff delay.
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
        }
    }
}

impl RetryPolicy {
    /// Constructs a new `RetryPolicy`.
    pub fn new(max_attempts: u32, base_delay: Duration, max_delay: Duration) -> Self {
        Self {
            max_attempts,
            base_delay,
            max_delay,
        }
    }

    /// Computes the delay for a given attempt index (0-based) using exponential backoff
    /// and ±25% random jitter, or returns the `retry_after` duration if provided.
    pub fn compute_delay(&self, attempt: u32, retry_after: Option<Duration>) -> Duration {
        if let Some(ra) = retry_after {
            return ra;
        }

        let factor = 2u64.saturating_pow(attempt);
        let exp_millis = (self.base_delay.as_millis() as u64).saturating_mul(factor);
        let capped_millis = exp_millis.min(self.max_delay.as_millis() as u64);

        let jitter = random_jitter_ratio();
        let jittered_millis = ((capped_millis as f64) * jitter).max(0.0) as u64;
        Duration::from_millis(jittered_millis)
    }
}

fn random_jitter_ratio() -> f64 {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(42);
    // Maps nanos to range [0.75, 1.25] (±25% jitter)
    0.75 + ((nanos % 1000) as f64 / 1000.0) * 0.50
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_policy_defaults() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_attempts, 3);
        assert_eq!(policy.base_delay, Duration::from_millis(500));
        assert_eq!(policy.max_delay, Duration::from_secs(30));
    }

    #[test]
    fn test_retry_policy_retry_after_override() {
        let policy = RetryPolicy::default();
        let delay = policy.compute_delay(0, Some(Duration::from_secs(5)));
        assert_eq!(delay, Duration::from_secs(5));
    }

    #[test]
    fn test_retry_policy_exponential_backoff_bounds() {
        let policy = RetryPolicy::new(5, Duration::from_millis(100), Duration::from_secs(1));
        // Attempt 0: base 100ms * (0.75 .. 1.25) -> 75ms..125ms
        let d0 = policy.compute_delay(0, None);
        assert!(d0 >= Duration::from_millis(70) && d0 <= Duration::from_millis(130));

        // Attempt 1: base 200ms -> 150ms..250ms
        let d1 = policy.compute_delay(1, None);
        assert!(d1 >= Duration::from_millis(140) && d1 <= Duration::from_millis(260));

        // Attempt 10: capped at 1000ms -> 750ms..1250ms
        let d10 = policy.compute_delay(10, None);
        assert!(d10 >= Duration::from_millis(700) && d10 <= Duration::from_millis(1300));
    }
}
