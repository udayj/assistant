use base64::{engine::general_purpose, Engine as _};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use std::collections::HashMap;

type HmacSha1 = Hmac<Sha1>;

/// Validates Twilio webhook signature
/// Returns true if the signature is valid
pub fn validate_twilio_signature(
    signature: &str,
    url: &str,
    params: &HashMap<String, String>,
    auth_token: &str,
) -> bool {
    // Remove "sha1=" prefix from signature if present
    let signature = signature.strip_prefix("sha1=").unwrap_or(signature);

    // Decode the expected signature from base64
    let expected_signature = match general_purpose::STANDARD.decode(signature) {
        Ok(sig) => sig,
        Err(_) => return false,
    };

    // Build the data string that Twilio signs
    let data = build_twilio_data_string(url, params);

    // Create HMAC-SHA1 with auth token
    let mut mac = match HmacSha1::new_from_slice(auth_token.as_bytes()) {
        Ok(mac) => mac,
        Err(_) => return false,
    };

    mac.update(data.as_bytes());

    // Verify the signature
    mac.verify_slice(&expected_signature).is_ok()
}

/// Builds the data string that Twilio signs
/// Format: URL + sorted form parameters
fn build_twilio_data_string(url: &str, params: &HashMap<String, String>) -> String {
    let mut data = url.to_string();

    // Sort parameters by key
    let mut sorted_params: Vec<_> = params.iter().collect();
    sorted_params.sort_by_key(|&(key, _)| key);

    // Append each parameter as key=value
    for (key, value) in sorted_params {
        data.push_str(key);
        data.push_str(value);
    }

    data
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_build_data_string() {
        let mut params = HashMap::new();
        params.insert("From".to_string(), "whatsapp:+1234567890".to_string());
        params.insert("Body".to_string(), "Hello World".to_string());

        let url = "https://example.com/webhook";
        let data = build_twilio_data_string(url, &params);

        // Parameters should be sorted alphabetically
        assert_eq!(data, "https://example.com/webhookBodyHello WorldFromwhatsapp:+1234567890");
    }

    #[test]
    fn test_empty_params() {
        let params = HashMap::new();
        let url = "https://example.com/webhook";
        let data = build_twilio_data_string(url, &params);

        assert_eq!(data, "https://example.com/webhook");
    }
}