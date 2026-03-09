use tokio::time::{self, Duration};

use super::token_bucket::{CheckResult, TokenBucket};

#[test]
fn new_bucket_starts_full() {
    let bucket = TokenBucket::new(10, 120);
    assert_eq!(bucket.remaining(), 10);
    assert_eq!(bucket.capacity(), 10);
    assert_eq!(bucket.rpm_limit(), 120);
}

#[test]
fn check_consumes_token() {
    let mut bucket = TokenBucket::new(5, 60);
    match bucket.check() {
        CheckResult::Allowed { remaining } => assert_eq!(remaining, 4),
        CheckResult::Denied { .. } => panic!("expected allowed"),
    }
}

#[test]
fn burst_exhaustion_denies() {
    let mut bucket = TokenBucket::new(2, 60);

    assert!(matches!(bucket.check(), CheckResult::Allowed { .. }));
    assert!(matches!(bucket.check(), CheckResult::Allowed { .. }));

    match bucket.check() {
        CheckResult::Denied { retry_after_secs } => {
            assert!(retry_after_secs >= 1, "retry_after should be at least 1s");
        }
        CheckResult::Allowed { .. } => panic!("expected denied after burst exhaustion"),
    }
}

#[tokio::test]
async fn tokens_refill_over_time() {
    time::pause();

    let mut bucket = TokenBucket::new(5, 60); // 1 token/sec

    for _ in 0..5 {
        assert!(matches!(bucket.check(), CheckResult::Allowed { .. }));
    }
    assert!(matches!(bucket.check(), CheckResult::Denied { .. }));

    // Advance 3 seconds -> 3 tokens refilled.
    time::advance(Duration::from_secs(3)).await;

    match bucket.check() {
        CheckResult::Allowed { remaining } => {
            assert_eq!(remaining, 2);
        }
        CheckResult::Denied { .. } => panic!("expected allowed after refill"),
    }
}

#[tokio::test]
async fn tokens_capped_at_capacity() {
    time::pause();

    let mut bucket = TokenBucket::new(3, 120); // 2 tokens/sec, capacity 3

    assert!(matches!(bucket.check(), CheckResult::Allowed { .. }));

    // Wait a very long time — tokens should cap at capacity.
    time::advance(Duration::from_secs(100)).await;

    match bucket.check() {
        CheckResult::Allowed { remaining } => assert_eq!(remaining, 2),
        CheckResult::Denied { .. } => panic!("expected allowed"),
    }
}

#[tokio::test]
async fn continuous_refill() {
    time::pause();

    let mut bucket = TokenBucket::new(10, 120); // 2 tokens/sec

    for _ in 0..10 {
        assert!(matches!(bucket.check(), CheckResult::Allowed { .. }));
    }

    // After 0.5 seconds, 1 token should be available (2 * 0.5 = 1).
    time::advance(Duration::from_millis(500)).await;

    assert!(matches!(bucket.check(), CheckResult::Allowed { .. }));
    assert!(matches!(bucket.check(), CheckResult::Denied { .. }));
}

#[test]
fn retry_after_calculation() {
    let mut bucket = TokenBucket::new(1, 60); // 1 token/sec

    assert!(matches!(bucket.check(), CheckResult::Allowed { .. }));

    match bucket.check() {
        CheckResult::Denied { retry_after_secs } => assert_eq!(retry_after_secs, 1),
        CheckResult::Allowed { .. } => panic!("expected denied"),
    }
}

#[test]
fn zero_capacity_always_denies() {
    let mut bucket = TokenBucket::new(0, 0);
    assert!(matches!(bucket.check(), CheckResult::Denied { .. }));
}
