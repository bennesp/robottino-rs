use hmac::{Hmac, Mac};
use md5::{Digest, Md5};
use sha2::Sha256;

/// Whitelist of parameters included in the sign string.
const SIGN_PARAMS: &[&str] = &[
    "a",
    "v",
    "lat",
    "lon",
    "lang",
    "deviceId",
    "appVersion",
    "ttid",
    "isH5",
    "h5Token",
    "os",
    "clientId",
    "postData",
    "time",
    "requestId",
    "et",
    "n4h5",
    "sid",
    "chKey",
    "sp",
];

/// MD5 hash with Tuya's hex block swap for postData.
///
/// Computes MD5, then rearranges the hex blocks: `[8:16]+[0:8]+[24:32]+[16:24]`.
pub fn post_data_hash_transform(post_data: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(post_data.as_bytes());
    let md5_hex = format!("{:x}", hasher.finalize());

    // Block swap: [8:16] + [0:8] + [24:32] + [16:24]
    let mut result = String::with_capacity(32);
    result.push_str(&md5_hex[8..16]);
    result.push_str(&md5_hex[0..8]);
    result.push_str(&md5_hex[24..32]);
    result.push_str(&md5_hex[16..24]);
    result
}

/// Build sign string from parameters.
///
/// Sorts whitelisted params alphabetically, joins with `||`.
/// Empty/missing values are skipped. `postData` is hashed with block swap.
pub fn build_sign_string(params: &[(&str, &str)]) -> String {
    let mut filtered: Vec<(&str, String)> = params
        .iter()
        .filter(|(k, v)| SIGN_PARAMS.contains(k) && !v.is_empty())
        .map(|(k, v)| {
            if *k == "postData" {
                (*k, post_data_hash_transform(v))
            } else {
                (*k, v.to_string())
            }
        })
        .collect();

    filtered.sort_by_key(|(k, _)| *k);

    filtered
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("||")
}

/// Compute HMAC-SHA256 signature, returning lowercase hex.
pub fn compute_sign(sign_string: &str, hmac_key: &str) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(hmac_key.as_bytes()).expect("HMAC accepts any key length");
    mac.update(sign_string.as_bytes());
    let result = mac.finalize();
    crate::crypto::hex_encode(&result.into_bytes())
}

/// Build the HMAC key from app credentials.
///
/// Format: `{package_name}_{cert_hash}_{bmp_key}_{app_secret}`
pub fn build_hmac_key(
    package_name: &str,
    cert_hash: &str,
    bmp_key: &str,
    app_secret: &str,
) -> String {
    format!("{package_name}_{cert_hash}_{bmp_key}_{app_secret}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACKAGE_NAME: &str = "com.example.test.app";
    const CERT_HASH: &str = "AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99";
    const BMP_KEY: &str = "testbmpkey0123456789abcdefghijkl";
    const APP_SECRET: &str = "testsecret0123456789abcdefghijkl";

    fn test_hmac_key() -> String {
        build_hmac_key(PACKAGE_NAME, CERT_HASH, BMP_KEY, APP_SECRET)
    }

    #[test]
    fn hmac_key_format() {
        let key = test_hmac_key();
        assert!(key.starts_with("com.example.test.app_AA:BB:"));
        assert!(key.ends_with("_testsecret0123456789abcdefghijkl"));
        assert!(key.contains("_testbmpkey0123456789abcdefghijkl_"));
    }

    #[test]
    fn post_data_hash_transform_deterministic() {
        let hash = post_data_hash_transform("{}");
        assert_eq!(hash.len(), 32);
        // Verify it's different from plain MD5 (block swap)
        let mut hasher = md5::Md5::new();
        hasher.update(b"{}");
        let plain = format!("{:x}", hasher.finalize());
        assert_ne!(hash, plain);
        // But contains the same chars rearranged
        let mut h_sorted: Vec<char> = hash.chars().collect();
        let mut p_sorted: Vec<char> = plain.chars().collect();
        h_sorted.sort();
        p_sorted.sort();
        assert_eq!(h_sorted, p_sorted);
    }

    #[test]
    fn build_sign_string_sorts_and_filters() {
        let params = vec![
            ("a", "tuya.m.location.list"),
            ("v", "1.0"),
            ("clientId", "test_client_id_placeholder"),
            ("notInWhitelist", "ignored"),
            ("os", "Android"),
        ];
        let ss = build_sign_string(&params);
        // Should be sorted alphabetically: a, clientId, os, v
        assert!(ss.starts_with("a=tuya.m.location.list||"));
        assert!(ss.contains("clientId=test_client_id_placeholder"));
        assert!(ss.contains("os=Android"));
        assert!(ss.ends_with("v=1.0"));
        assert!(!ss.contains("notInWhitelist"));
    }

    #[test]
    fn build_sign_string_skips_empty() {
        let params = vec![("a", "test"), ("sid", ""), ("v", "1.0")];
        let ss = build_sign_string(&params);
        assert!(!ss.contains("sid"));
    }

    #[test]
    fn build_sign_string_hashes_post_data() {
        let params = vec![("a", "test"), ("postData", "{\"foo\":\"bar\"}")];
        let ss = build_sign_string(&params);
        // postData value should be 32-char hex hash, not the original JSON
        assert!(!ss.contains("{\"foo\""));
        let pd_part = ss.split("postData=").nth(1).unwrap();
        assert_eq!(pd_part.len(), 32);
    }

    #[test]
    fn compute_sign_deterministic() {
        // Synthetic sign string with fake values to verify HMAC computation is stable.
        let sign_string = "a=tuya.m.device.list||appVersion=1.0.0||clientId=test_client_id_placeholder||deviceId=0000000000000000000000000000000000000000000000||lang=en_US||os=Android||postData=ba1989685bb02d6d6669c58707a2d23c||sid=test_session_placeholder||time=1700000000||v=1.0";
        let hmac_key = test_hmac_key();

        let result = compute_sign(sign_string, &hmac_key);
        assert_eq!(result.len(), 64, "HMAC-SHA256 should produce 64 hex chars");
        // Verify determinism: calling again gives same result.
        let result2 = compute_sign(sign_string, &hmac_key);
        assert_eq!(result, result2);
        // Pinned expected value — update if key or sign string changes.
        assert_eq!(
            result,
            "85d87ee424153603639cf50ee75949a4655c9807807414c293461d1747684868"
        );
    }
}
