use std::path::Path;

use anyhow::Context;
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use miden_pow_rate_limiter::ChallengeError;
use rand::RngExt;
use serde::{Deserialize, Serialize};

// API KEY
// ================================================================================================

const API_KEY_PREFIX: &str = "miden_faucet_";

/// The API key is a random 32-byte array.
///
/// It can be encoded as a string using the `encode` method and decoded back to bytes using the
/// `decode` method.
#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct ApiKey([u8; 32]);

impl ApiKey {
    /// Generates a random API key.
    pub fn generate(rng: &mut impl RngExt) -> Self {
        let mut api_key = [0u8; 32];
        rng.fill(&mut api_key);
        Self(api_key)
    }

    /// Creates an API key from a byte array.
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Encodes the API key into a base64 string, prefixed with `API_KEY_PREFIX`.
    pub fn encode(&self) -> String {
        format!("{API_KEY_PREFIX}{}", BASE64_STANDARD.encode(self.0))
    }

    /// Decodes the API key from a string.
    pub fn decode(api_key_str: &str) -> Result<Self, ChallengeError> {
        let api_key_str = api_key_str.trim_start_matches(API_KEY_PREFIX).to_string();
        let bytes = BASE64_STANDARD
            .decode(api_key_str.as_bytes())
            .map_err(|_| ChallengeError::InvalidDomain(api_key_str.clone()))?;

        let api_key =
            Self(bytes.try_into().map_err(|_| ChallengeError::InvalidDomain(api_key_str))?);
        Ok(api_key)
    }
}

impl From<ApiKey> for [u8; 32] {
    fn from(api_key: ApiKey) -> Self {
        api_key.0
    }
}

// API KEYS
// ================================================================================================

/// The API keys stored in a newline-delimited file of encoded keys.
///
/// Holds no state: the file path is passed to each operation.
pub struct ApiKeys;

impl ApiKeys {
    /// Reads the encoded API keys from `path`, which must exist.
    pub async fn list(path: &Path) -> anyhow::Result<Vec<String>> {
        let contents = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("failed to read {}", path.display()))?;

        Ok(contents
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(String::from)
            .collect())
    }

    /// Reads and decodes the API keys from `path`, which must exist.
    pub async fn load(path: &Path) -> anyhow::Result<Vec<ApiKey>> {
        Self::list(path)
            .await?
            .iter()
            .map(|encoded| ApiKey::decode(encoded).map_err(|error| anyhow::anyhow!(error)))
            .collect()
    }

    /// Adds `key` to the file at `path`, creating it if it does not exist.
    pub async fn add(path: &Path, key: &ApiKey) -> anyhow::Result<()> {
        // The first key is created before the file exists.
        let mut keys = if path.exists() { Self::list(path).await? } else { Vec::new() };
        let encoded = key.encode();
        if !keys.contains(&encoded) {
            keys.push(encoded);
        }

        Self::write(path, &keys).await
    }

    /// Removes `key` from the file at `path`.
    ///
    /// Fails if the key is not present, so a typo does not report a successful removal.
    pub async fn remove(path: &Path, key: &ApiKey) -> anyhow::Result<()> {
        let mut keys = Self::list(path).await?;
        let encoded = key.encode();
        let before = keys.len();
        keys.retain(|existing| existing != &encoded);
        anyhow::ensure!(keys.len() < before, "API key not found in {}", path.display());

        Self::write(path, &keys).await
    }

    async fn write(path: &Path, keys: &[String]) -> anyhow::Result<()> {
        let mut contents = keys.join("\n");
        contents.push('\n');
        tokio::fs::write(path, contents)
            .await
            .with_context(|| format!("failed to write {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;
    use rand::rngs::ChaCha20Rng;

    use super::*;
    use crate::api_key::{API_KEY_PREFIX, ApiKey};

    #[test]
    fn api_key_encode_and_decode() {
        let mut rng = ChaCha20Rng::from_seed(rand::random());
        let api_key = ApiKey::generate(&mut rng);

        let encoded_key = api_key.encode();
        assert!(encoded_key.starts_with(API_KEY_PREFIX));

        let decoded_key = ApiKey::decode(&encoded_key).unwrap();
        assert_eq!(decoded_key.0.len(), 32);
        assert_eq!(decoded_key.0, api_key.0);
    }

    /// A round trip through the file: added keys are read back, a removed key is gone, and
    /// removing a key that is not there fails.
    #[tokio::test]
    async fn api_keys_round_trip_through_the_file() {
        let path = std::env::temp_dir().join(format!("{}.keys", uuid::Uuid::new_v4()));
        let mut rng = ChaCha20Rng::from_seed(rand::random());
        let first = ApiKey::generate(&mut rng);
        let second = ApiKey::generate(&mut rng);

        ApiKeys::load(&path).await.expect_err("a missing file should fail to load");

        ApiKeys::add(&path, &first).await.unwrap();
        ApiKeys::add(&path, &second).await.unwrap();
        assert_eq!(ApiKeys::load(&path).await.unwrap(), vec![first.clone(), second.clone()]);

        ApiKeys::remove(&path, &first).await.unwrap();
        assert_eq!(ApiKeys::load(&path).await.unwrap(), vec![second.clone()]);

        ApiKeys::remove(&path, &first).await.expect_err("removing an absent key should fail");
    }
}
