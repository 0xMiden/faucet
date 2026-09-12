use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::challenge_cache::ChallengeCache;

mod challenge;
mod challenge_cache;
#[cfg(test)]
mod tests;

pub use challenge::Challenge;

// PoW Rate Limiter
// ================================================================================================

/// Proof-of-Work Rate Limiter implementation.
///
/// This struct is used to enforce rate limiting based on `PoW` challenges.
#[derive(Clone)]
pub struct PoWRateLimiter {
    /// The server secret used to sign and validate challenges.
    secret: [u8; 32],
    /// The cache used to store submitted challenges.
    challenges: Arc<RwLock<ChallengeCache>>,
    /// The settings of the rate limiter.
    config: PoWRateLimiterConfig,
}

/// Represents the requestor of a challenge.
type Requestor = [u8; 32];

/// Represents the domain of a challenge, which is a unique identifier for the service that is
/// requesting a challenge.
type Domain = [u8; 32];

/// The settings of `PoWRateLimiter`.
#[derive(Clone)]
pub struct PoWRateLimiterConfig {
    /// The lifetime for challenges. After this time, challenges are considered expired.
    pub challenge_lifetime: Duration,
    /// Determines how much the difficulty increases with the number of active challenges.
    pub growth_rate: f64,
    /// Sets the baseline difficulty bits when there are no active challenges.
    pub baseline: u8,
    /// The interval at which the challenge cache is cleaned up. Only expired challenges are
    /// removed during cleanup.
    pub cleanup_interval: Duration,
}

impl PoWRateLimiter {
    /// Creates a new `PoW` instance and starts a `tokio` task that periodically cleans up expired
    /// challenges.
    #[cfg(feature = "tokio")]
    pub fn new_with_cleanup(secret: [u8; 32], config: PoWRateLimiterConfig) -> Self {
        let cleanup_interval = config.cleanup_interval;
        let rate_limiter = Self::new(secret, config);
        let challenge_cache = rate_limiter.challenges.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(cleanup_interval);

            loop {
                interval.tick().await;
                remove_expired_challenges(&challenge_cache);
            }
        });

        rate_limiter
    }

    /// Creates a new `PoW` instance.
    ///
    /// Note: You need to manually run the cleanup task by calling `ChallengeCache::run_cleanup`
    /// periodically.
    ///
    /// See `PoWRateLimiter::new_with_cleanup` to instantiate the rate limiter with the cleanup task
    /// started automatically. It requires to enable the `tokio` feature.
    pub fn new(secret: [u8; 32], config: PoWRateLimiterConfig) -> Self {
        let challenge_cache = Arc::new(RwLock::new(ChallengeCache::new(config.challenge_lifetime)));
        Self {
            secret,
            challenges: challenge_cache,
            config,
        }
    }

    /// Cleans up the challenge cache, removing all the expired challenges.
    pub fn cleanup_challenge_cache(&self) {
        remove_expired_challenges(&self.challenges);
    }

    /// Generates a new challenge with a difficulty that will depend on the number of active
    /// challenges for the given domain and the request complexity.
    ///
    /// # Arguments
    /// * `requestor` - A unique identifier for the user that is requesting the challenge.
    /// * `domain` - A unique identifier for the service that is requesting the challenge.
    /// * `request_complexity` - A measure of the complexity of the request. Must be greater than 0.
    ///
    /// # Panics
    /// Panics if the request complexity is 0.
    pub fn build_challenge(
        &self,
        requestor: impl Into<Requestor>,
        domain: impl Into<Domain>,
        request_complexity: u64,
    ) -> Challenge {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("current timestamp should be greater than unix epoch")
            .as_secs();
        let requestor = requestor.into();
        let domain = domain.into();
        let target = self.get_challenge_target(&domain, request_complexity);

        Challenge::new(target, current_time, request_complexity, requestor, domain, self.secret)
    }

    /// Returns the load difficulty for a given domain.
    ///
    /// The load difficulty is computed as:
    /// `2^baseline * max(1, ceil((num_active_challenges + 1) * growth_rate))`
    ///
    /// The multiplier is clamped to at least `1` so that a non-positive (or `NaN`) growth rate
    /// degrades to a constant `2^baseline` difficulty instead of a zero difficulty, which the
    /// challenge target could not be derived from.
    pub fn get_load_difficulty(&self, domain: impl Into<Domain>) -> u64 {
        let num_challenges = self
            .challenges
            .read()
            .expect("challenge cache lock should not be poisoned")
            .num_challenges_for_domain(&domain.into());

        #[allow(clippy::cast_precision_loss, reason = "num_challenges is smaller than f64::MAX")]
        #[allow(
            clippy::cast_sign_loss,
            reason = "a negative product saturates to 0 and is clamped to 1 below"
        )]
        let growth_multiplier =
            (((num_challenges + 1) as f64 * self.config.growth_rate).ceil() as u64).max(1);
        2_u64.pow(self.config.baseline.into()).saturating_mul(growth_multiplier)
    }

    /// Computes the target for a given domain by checking the amount of active challenges in the
    /// cache and the given request complexity.
    ///
    /// The target is computed as: `target = u64::MAX / request_difficulty`
    ///
    /// Where:
    /// * `request_difficulty = load_difficulty * request_complexity`
    /// * `load_difficulty = 2^baseline * max(1, ceil((num_active_challenges + 1) * growth_rate))`
    fn get_challenge_target(&self, domain: &Domain, request_complexity: u64) -> u64 {
        if request_complexity == 0 {
            return u64::MAX;
        }
        let load_difficulty = self.get_load_difficulty(*domain);
        let request_difficulty = load_difficulty.saturating_mul(request_complexity);
        u64::MAX / request_difficulty
    }

    /// Submits a challenge.
    ///
    /// The challenge is validated and added to the cache.
    ///
    /// # Errors
    /// Returns an error if:
    /// * The challenge is expired.
    /// * The challenge is invalid.
    /// * The challenge was already used.
    /// * The requestor has already submitted a challenge recently and it's not expired yet.
    ///
    /// # Panics
    /// Panics if the challenge cache lock is poisoned.
    pub fn submit_challenge(
        &self,
        requestor: impl Into<Requestor>,
        domain: impl Into<Domain>,
        challenge: &Challenge,
        nonce: u64,
        current_time: u64,
        request_complexity: u64,
    ) -> Result<(), ChallengeError> {
        challenge.verify_signature(self.secret)?;

        let requestor = requestor.into();
        let domain = domain.into();

        // Check timestamp validity
        if challenge.is_expired(current_time, self.config.challenge_lifetime) {
            return Err(ChallengeError::ExpiredServerTimestamp(challenge.timestamp, current_time));
        }

        // Validate the challenge
        let valid_requestor = requestor == challenge.requestor;
        let valid_domain = domain == challenge.domain;
        let valid_nonce = challenge.validate_pow(nonce);
        let valid_request_complexity = challenge.request_complexity == request_complexity;
        if !(valid_nonce && valid_requestor && valid_domain && valid_request_complexity) {
            return Err(ChallengeError::InvalidPoW);
        }

        // Insert the challenge into the cache
        self.challenges
            .write()
            .expect("challenge cache lock should not be poisoned")
            .insert_challenge(challenge, current_time)?;

        Ok(())
    }
}

fn remove_expired_challenges(challenge_cache: &Arc<RwLock<ChallengeCache>>) {
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("current timestamp should be greater than unix epoch")
        .as_secs();
    challenge_cache
        .write()
        .expect("challenge cache lock should not be poisoned")
        .drop_expired_challenges(current_time);
}

/// `PoW` challenge related errors.
#[derive(Debug, thiserror::Error)]
pub enum ChallengeError {
    #[error("challenge timestamp expired, received: {0}, current time: {1}")]
    ExpiredServerTimestamp(u64, u64),
    #[error("invalid PoW solution")]
    InvalidPoW,
    #[error("requestor is rate limited for {0} more seconds")]
    RateLimited(u64),
    #[error("invalid challenge signature")]
    InvalidSignature,
    #[error("invalid challenge serialization")]
    InvalidSerialization,
    #[error("domain {0} is invalid")]
    InvalidDomain(String),
}
