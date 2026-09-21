use sha2::{Digest, Sha256};

async fn request_challenge(
    base_url: &str,
    account_address: &str,
) -> anyhow::Result<serde_json::Value> {
    let url = format!("{base_url}/pow?account_id={account_address}");
    let response = reqwest::get(&url).await?.error_for_status()?;
    let text = response.text().await?;
    let json: serde_json::Value = serde_json::from_str(&text)?;
    Ok(json)
}

fn solve_challenge(challenge: &str, target: u64) -> u64 {
    let mut found_nonce = None;
    let challenge_bytes = hex::decode(challenge).unwrap();

    for nonce in 0..u64::MAX {
        // Create SHA-256 hash
        let mut hasher = Sha256::new();
        hasher.update(challenge_bytes);
        hasher.update(nonce.to_be_bytes());
        let hash = hasher.finalize();

        // Take the first 8 bytes and interpret as big-endian u64
        let number = u64::from_be_bytes(hash[..8].try_into().unwrap());

        // Check if hash number is less than target
        if number < target {
            found_nonce = Some(nonce);
            break;
        }
    }
    found_nonce.expect("No valid nonce found")
}

async fn request_tokens(
    base_url: &str,
    account_address: &str,
    challenge: &str,
    nonce: u64,
    asset_amount: u64,
) -> anyhow::Result<serde_json::Value> {
    let params = [
        ("account_id", account_address),
        ("asset_amount", &asset_amount.to_string()),
        ("challenge", challenge),
        ("nonce", &nonce.to_string()),
    ]
    .iter()
    .map(|(key, value)| format!("{key}={value}"))
    .collect::<Vec<_>>()
    .join("&");
    let url = format!("{base_url}/get_tokens?{params}");
    let response = reqwest::get(&url).await?.error_for_status()?;
    let text = response.text().await?;
    let json: serde_json::Value = serde_json::from_str(&text)?;
    Ok(json)
}

#[tokio::main]
async fn main() {
    // This example assumes you have the faucet running on http://localhost:8000
    let account_address = "0xca8203e8e58cf72049b061afca78ce";
    let asset_amount = 100;
    let url = "http://localhost:8000";

    // Step 1: request challenge
    let challenge_response = request_challenge(url, account_address).await.unwrap();
    let challenge = challenge_response["challenge"].as_str().unwrap();
    let target = challenge_response["target"].as_u64().unwrap();

    // Step 2: solve challenge
    let nonce = solve_challenge(challenge, target);

    // Step 3: request tokens
    let result = request_tokens(url, account_address, challenge, nonce, asset_amount)
        .await
        .unwrap();
    println!("Token minted successfully:");
    println!("* Transaction ID: {}", result["tx_id"]);
    println!("* Note ID: {}", result["note_id"]);
}
