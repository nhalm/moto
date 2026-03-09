//! Token bucket rate limiter with continuous refill.

use tokio::time::Instant;

/// A token bucket that allows `capacity` burst requests and refills at
/// `refill_rate` tokens per second (RPM / 60).
///
/// Tokens refill continuously based on elapsed time since the last check.
/// The bucket starts full.
pub struct TokenBucket {
    /// Maximum number of tokens (burst size).
    capacity: f64,
    /// Tokens added per second (RPM / 60).
    refill_rate: f64,
    /// Current token count.
    tokens: f64,
    /// Last time tokens were calculated.
    last_refill: Instant,
    /// Last time the bucket was accessed (for cleanup/eviction).
    last_access: Instant,
}

/// Result of checking the token bucket.
#[derive(Debug)]
pub enum CheckResult {
    /// Request is allowed. Contains current token count after consuming one.
    Allowed {
        /// Remaining tokens (floored to integer for display).
        remaining: u64,
    },
    /// Request is denied. Contains retry-after in seconds.
    Denied {
        /// Seconds until at least one token is available.
        retry_after_secs: u64,
    },
}

/// Compute refilled tokens from elapsed time, capped at capacity.
fn refill(tokens: f64, elapsed_secs: f64, refill_rate: f64, capacity: f64) -> f64 {
    elapsed_secs.mul_add(refill_rate, tokens).min(capacity)
}

/// Return current Unix timestamp in seconds.
fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "token counts and rates are always non-negative and fit in u32/u64"
)]
impl TokenBucket {
    /// Create a new token bucket.
    ///
    /// - `capacity`: burst size (max tokens).
    /// - `rpm`: requests per minute. Refill rate = rpm / 60 tokens/sec.
    ///
    /// The bucket starts full (tokens = capacity).
    #[must_use]
    pub fn new(capacity: u32, rpm: u32) -> Self {
        let now = Instant::now();
        Self {
            capacity: f64::from(capacity),
            refill_rate: f64::from(rpm) / 60.0,
            tokens: f64::from(capacity),
            last_refill: now,
            last_access: now,
        }
    }

    /// Check if a request is allowed, consuming one token if so.
    ///
    /// Updates the token count based on elapsed time since the last check
    /// (continuous refill), then attempts to consume one token.
    pub fn check(&mut self) -> CheckResult {
        let now = Instant::now();
        self.last_access = now;

        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = refill(self.tokens, elapsed, self.refill_rate, self.capacity);
        self.last_refill = now;

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            CheckResult::Allowed {
                remaining: self.tokens as u64,
            }
        } else {
            let tokens_needed = 1.0 - self.tokens;
            let wait_secs = tokens_needed / self.refill_rate;
            CheckResult::Denied {
                retry_after_secs: wait_secs.ceil() as u64,
            }
        }
    }

    /// Returns the last time this bucket was accessed.
    #[must_use]
    pub const fn last_access(&self) -> Instant {
        self.last_access
    }

    /// Returns the current RPM limit for this bucket.
    #[must_use]
    pub fn rpm_limit(&self) -> u32 {
        (self.refill_rate * 60.0) as u32
    }

    /// Returns the current number of remaining tokens (approximate).
    ///
    /// This peeks at the current state without consuming a token or updating
    /// the last access time.
    #[must_use]
    pub fn remaining(&self) -> u64 {
        let elapsed = self.last_refill.elapsed().as_secs_f64();
        refill(self.tokens, elapsed, self.refill_rate, self.capacity) as u64
    }

    /// Returns the Unix timestamp when the bucket will be full again.
    #[must_use]
    pub fn reset_at(&self) -> u64 {
        let elapsed = self.last_refill.elapsed().as_secs_f64();
        let current_tokens = refill(self.tokens, elapsed, self.refill_rate, self.capacity);

        if current_tokens >= self.capacity {
            return now_unix_secs();
        }

        let tokens_needed = self.capacity - current_tokens;
        let secs_to_full = tokens_needed / self.refill_rate;
        now_unix_secs() + secs_to_full.ceil() as u64
    }

    /// Returns the capacity (burst size) of this bucket.
    #[must_use]
    pub const fn capacity(&self) -> u32 {
        self.capacity as u32
    }
}
