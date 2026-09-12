use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::{Challenge, ChallengeError, PoWRateLimiter, PoWRateLimiterConfig};

fn find_pow_solution(challenge: &Challenge, max_iterations: u64) -> Option<u64> {
    (0..max_iterations).find(|&nonce| challenge.validate_pow(nonce))
}

fn create_test_pow() -> PoWRateLimiter {
    let mut secret = [0u8; 32];
    secret[..12].copy_from_slice(b"miden-faucet");

    PoWRateLimiter::new_with_cleanup(
        secret,
        PoWRateLimiterConfig {
            challenge_lifetime: Duration::from_secs(3),
            growth_rate: 1.0,
            cleanup_interval: Duration::from_millis(500),
            baseline: 0,
        },
    )
}

#[tokio::test]
async fn challenge_nonce_is_validated() {
    let pow = create_test_pow();
    let domain = [1u8; 32];
    let current_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let requestor = [0u8; 32];
    let request_complexity = 1;
    let challenge = pow.build_challenge(requestor, domain, request_complexity);
    let nonce = find_pow_solution(&challenge, 10000).expect("Should find solution");

    // Submit challenge with correct nonce - should succeed
    let result = pow.submit_challenge(
        requestor,
        domain,
        &challenge,
        nonce,
        current_time,
        request_complexity,
    );
    assert!(result.is_ok());

    // Try to use the same challenge again with another requestor - should fail
    let requestor = [1u8; 32];
    let result = pow.submit_challenge(
        requestor,
        domain,
        &challenge,
        nonce,
        current_time,
        request_complexity,
    );
    assert!(result.is_err());
}

#[tokio::test]
async fn challenge_timestamp_is_validated() {
    let pow = create_test_pow();
    let domain = [1u8; 32];
    let requestor = [0u8; 32];
    let request_complexity = 1;
    let current_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();

    let challenge = pow.build_challenge(requestor, domain, request_complexity);
    let nonce = find_pow_solution(&challenge, 10000).expect("Should find solution");

    // Submit challenge with expired timestamp - should fail
    let result = pow.submit_challenge(
        requestor,
        domain,
        &challenge,
        nonce,
        current_time + pow.config.challenge_lifetime.as_secs() + 1,
        request_complexity,
    );
    assert!(result.is_err());

    // Submit challenge with correct timestamp - should succeed
    let result = pow.submit_challenge(
        requestor,
        domain,
        &challenge,
        nonce,
        current_time,
        request_complexity,
    );
    assert!(result.is_ok());
}

#[tokio::test]
async fn requestor_is_rate_limited() {
    let pow = create_test_pow();
    let domain = [1u8; 32];
    let requestor = [0u8; 32];
    let request_complexity = 1;

    // Solve first challenge
    let challenge = pow.build_challenge(requestor, domain, request_complexity);
    let nonce = find_pow_solution(&challenge, 10000).expect("Should find solution");

    let time_1 = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let result =
        pow.submit_challenge(requestor, domain, &challenge, nonce, time_1, request_complexity);
    assert!(result.is_ok());

    // Try to submit second challenge - should fail because of rate limiting
    let challenge = pow.build_challenge(requestor, domain, request_complexity);
    let nonce = find_pow_solution(&challenge, 10000).expect("Should find solution");

    let time_2 = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let result =
        pow.submit_challenge(requestor, domain, &challenge, nonce, time_2, request_complexity);
    assert!(result.is_err());
    let remaining_time = time_1 + pow.config.challenge_lifetime.as_secs() - time_2;
    let Err(ChallengeError::RateLimited(timestamp)) = result else {
        panic!("Expected RateLimited error");
    };
    assert_eq!(timestamp, remaining_time);

    // Try to submit it using a different api key - should succeed
    let domain = [2u8; 32];
    let challenge = pow.build_challenge(requestor, domain, request_complexity);
    let nonce = find_pow_solution(&challenge, 10000).expect("Should find solution");
    let result =
        pow.submit_challenge(requestor, domain, &challenge, nonce, time_2, request_complexity);
    assert!(result.is_ok());
}

#[tokio::test]
async fn requestor_is_rate_limited_after_challenge_expires() {
    let mut secret = [0u8; 32];
    secret[..12].copy_from_slice(b"miden-faucet");
    let challenge_lifetime = Duration::from_secs(3);

    // Drive the test with explicit timestamps.
    let pow = PoWRateLimiter::new(
        secret,
        PoWRateLimiterConfig {
            challenge_lifetime,
            growth_rate: 1.0,
            cleanup_interval: Duration::from_secs(3),
            baseline: 0,
        },
    );

    let domain = [1u8; 32];
    let requestor = [0u8; 32];
    let request_complexity = 1;
    let lifetime_secs = challenge_lifetime.as_secs();
    let issued_at = 1_000_000;

    // Solve challenge 1, and challenge 2 issued one second later.
    let target = pow.get_challenge_target(&domain, request_complexity);
    let challenge_1 =
        Challenge::new(target, issued_at, request_complexity, requestor, domain, secret);
    let nonce_1 = find_pow_solution(&challenge_1, 10000).expect("Should find solution");
    let challenge_2 =
        Challenge::new(target, issued_at + 1, request_complexity, requestor, domain, secret);
    let nonce_2 = find_pow_solution(&challenge_2, 10000).expect("Should find solution");

    // Submit challenge 1 on the last second on which it is still valid.
    let time_1 = issued_at + lifetime_secs - 1;
    let result =
        pow.submit_challenge(requestor, domain, &challenge_1, nonce_1, time_1, request_complexity);
    assert!(result.is_ok());

    // One second later challenge 1 has expired, but submitting challenge 2 still fails.
    let time_2 = time_1 + 1;
    assert!(challenge_1.is_expired(time_2, challenge_lifetime));
    let result =
        pow.submit_challenge(requestor, domain, &challenge_2, nonce_2, time_2, request_complexity);
    let Err(ChallengeError::RateLimited(remaining_time)) = result else {
        panic!("Expected RateLimited error");
    };
    assert_eq!(remaining_time, lifetime_secs - 1);
}

#[tokio::test]
async fn solved_challenge_cannot_be_submitted_twice() {
    let mut secret = [0u8; 32];
    secret[..12].copy_from_slice(b"miden-faucet");
    let challenge_lifetime = Duration::from_secs(30);

    // `PoWRateLimiter::new` does not start the cleanup task, so the cache is never cleaned up
    // during the test and `cleanup_interval` is unused: the reuse below has to be rejected by
    // challenge validation alone.
    let pow = PoWRateLimiter::new(
        secret,
        PoWRateLimiterConfig {
            challenge_lifetime,
            growth_rate: 1.0,
            cleanup_interval: Duration::from_secs(3),
            baseline: 0,
        },
    );

    let domain = [1u8; 32];
    let requestor = [0u8; 32];
    let request_complexity = 1;
    let issued_at = 1_000_000;

    let target = pow.get_challenge_target(&domain, request_complexity);
    let challenge =
        Challenge::new(target, issued_at, request_complexity, requestor, domain, secret);
    let nonce = find_pow_solution(&challenge, 10000).expect("Should find solution");

    // The solver redeems the challenge as soon as it is solved.
    let result =
        pow.submit_challenge(requestor, domain, &challenge, nonce, issued_at, request_complexity);
    assert!(result.is_ok());

    // The rate limit on this solver lifts exactly `challenge_lifetime` seconds after that
    // submission.
    let rate_limit_lifted_at = issued_at + challenge_lifetime.as_secs();
    let result = pow.submit_challenge(
        requestor,
        domain,
        &challenge,
        nonce,
        rate_limit_lifted_at,
        request_complexity,
    );
    assert!(matches!(result, Err(ChallengeError::ExpiredServerTimestamp(_, _))));
}

#[tokio::test]
async fn challenge_expires_once_its_full_lifetime_elapsed() {
    let secret = [1u8; 32];
    let domain = [1u8; 32];
    let requestor = [0u8; 32];
    let challenge_lifetime = Duration::from_secs(30);
    let issued_at = 1_000_000;

    let challenge = Challenge::new(u64::MAX, issued_at, 1, requestor, domain, secret);

    assert!(!challenge.is_expired(issued_at, challenge_lifetime));
    // Still valid on the last second of its lifetime.
    assert!(
        !challenge.is_expired(issued_at + challenge_lifetime.as_secs() - 1, challenge_lifetime)
    );
    // Expired once the full lifetime has elapsed.
    assert!(challenge.is_expired(issued_at + challenge_lifetime.as_secs(), challenge_lifetime));
}

#[tokio::test]
async fn difficuty_increases_with_submitted_challenges() {
    let pow = create_test_pow();
    let domain = [1u8; 32];
    let requestor = [0u8; 32];
    let current_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let request_complexity = 1;

    assert_eq!(
        pow.get_challenge_target(&domain, request_complexity),
        u64::MAX >> pow.config.baseline
    );

    let challenge = pow.build_challenge(requestor, domain, request_complexity);
    let nonce = find_pow_solution(&challenge, 10000).expect("Should find solution");

    pow.submit_challenge(requestor, domain, &challenge, nonce, current_time, request_complexity)
        .unwrap();

    assert_eq!(pow.challenges.read().unwrap().num_challenges_for_domain(&domain), 1);
    assert_eq!(
        pow.get_challenge_target(&domain, request_complexity),
        (u64::MAX >> pow.config.baseline) / 2
    );
}

#[tokio::test]
async fn difficulty_increases_with_request_complexity() {
    let pow = create_test_pow();
    let domain = [1u8; 32];

    // test: request complexity 1 should have difficulty 1
    let request_complexity = 1;

    let difficulty = 1;
    assert_eq!(
        pow.get_challenge_target(&domain, request_complexity),
        (u64::MAX >> pow.config.baseline) / difficulty
    );

    // test: request complexity 3 should have difficulty 3
    let request_complexity = 3;

    let difficulty = 3;
    assert_eq!(
        pow.get_challenge_target(&domain, request_complexity),
        (u64::MAX >> pow.config.baseline) / difficulty
    );
}

#[tokio::test]
async fn submit_challenge_while_previous_one_is_not_cleaned_up() {
    let mut secret = [0u8; 32];
    secret[..12].copy_from_slice(b"miden-faucet");

    // setup pow with short challenge lifetime and long cleanup interval to test the case
    // where cleanup has not run yet but the challenge is expired.
    let pow = PoWRateLimiter::new(
        secret,
        PoWRateLimiterConfig {
            challenge_lifetime: Duration::from_secs(1),
            growth_rate: 1.0,
            cleanup_interval: Duration::from_secs(3),
            baseline: 0,
        },
    );
    let domain_1 = [1u8; 32];
    let requestor = [0u8; 32];
    let request_complexity = 1;
    let current_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();

    // submit first challenge
    let challenge = pow.build_challenge(requestor, domain_1, request_complexity);
    let nonce = find_pow_solution(&challenge, 10000).expect("Should find solution");
    let result = pow.submit_challenge(
        requestor,
        domain_1,
        &challenge,
        nonce,
        current_time,
        request_complexity,
    );
    assert!(result.is_ok());

    // submit another challenge with same timestamp but different domain
    let domain_2 = [2u8; 32];
    let challenge = pow.build_challenge(requestor, domain_2, request_complexity);
    let nonce = find_pow_solution(&challenge, 10000).expect("Should find solution");
    let result = pow.submit_challenge(
        requestor,
        domain_2,
        &challenge,
        nonce,
        current_time,
        request_complexity,
    );
    assert!(result.is_ok());

    // submit challenge that overrides the first one (same solver but previous challenge is
    // expired)
    tokio::time::sleep(pow.config.challenge_lifetime + Duration::from_secs(1)).await;
    let current_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let challenge = pow.build_challenge(requestor, domain_1, request_complexity);
    let nonce = find_pow_solution(&challenge, 10000).expect("Should find solution");
    let result = pow.submit_challenge(
        requestor,
        domain_1,
        &challenge,
        nonce,
        current_time,
        request_complexity,
    );
    assert!(result.is_ok());

    // check that the first challenge is removed from the cache
    assert_eq!(pow.challenges.read().unwrap().num_challenges_for_domain(&domain_1), 1);
    assert_eq!(pow.challenges.read().unwrap().num_challenges_for_domain(&domain_2), 1);
}

/// A non-positive or `NaN` growth rate must not zero out the load difficulty: the challenge target
/// is `u64::MAX / difficulty`, so a zero difficulty would make `build_challenge` divide by zero.
#[tokio::test]
async fn non_positive_growth_rate_falls_back_to_baseline_difficulty() {
    for growth_rate in [0.0, -0.5, f64::NAN] {
        let pow = PoWRateLimiter::new_with_cleanup(
            [1u8; 32],
            PoWRateLimiterConfig {
                challenge_lifetime: Duration::from_secs(30),
                growth_rate,
                baseline: 4,
                cleanup_interval: Duration::from_secs(2),
            },
        );

        assert_eq!(pow.get_load_difficulty([0u8; 32]), 1 << 4, "growth rate {growth_rate}");
        let challenge = pow.build_challenge([0u8; 32], [0u8; 32], 1);
        assert_eq!(challenge.target(), u64::MAX >> 4, "growth rate {growth_rate}");
    }
}
