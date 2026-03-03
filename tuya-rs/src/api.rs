use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::signing;

/// API error types.
#[derive(Debug, Error)]
pub enum ApiError {
    /// Session expired, re-login required.
    #[error("session expired — need to re-login")]
    SessionInvalid,
    /// Wrong email or password.
    #[error("wrong email or password")]
    PasswordWrong,
    /// API action not available for this client.
    #[error("API action not available for this client")]
    IllegalAccessApi,
    /// HTTP network error.
    #[error("network error: {0}")]
    NetworkError(String),
    /// Server returned an error with a Tuya error code.
    #[error("server error {code}: {message}")]
    ServerError {
        /// Tuya error code.
        code: String,
        /// Error message from server.
        message: String,
    },
    /// Failed to parse API response.
    #[error("response parsing failed: {0}")]
    ParseError(String),
}

/// OEM app credentials extracted from APK + Ghidra.
#[derive(Debug, Clone)]
pub struct OemCredentials {
    /// Tuya app client ID.
    pub client_id: String,
    /// App secret key.
    pub app_secret: String,
    /// BMP signing key.
    pub bmp_key: String,
    /// APK certificate SHA-256 hash.
    pub cert_hash: String,
    /// Android package name.
    pub package_name: String,
    /// App installation device fingerprint, sent as `deviceId` in API requests.
    pub app_device_id: String,
}

impl OemCredentials {
    /// Build the HMAC key for API signing.
    pub fn hmac_key(&self) -> String {
        signing::build_hmac_key(
            &self.package_name,
            &self.cert_hash,
            &self.bmp_key,
            &self.app_secret,
        )
    }
}

/// Active API session.
#[derive(Debug, Clone)]
pub struct Session {
    /// Session ID.
    pub sid: String,
    /// User ID.
    pub uid: String,
    /// Account email.
    pub email: String,
    /// API endpoint domain.
    pub domain: String,
}

/// A discovered Tuya device.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// Tuya device ID.
    pub dev_id: String,
    /// AES local encryption key.
    pub local_key: String,
    /// Device display name.
    pub name: String,
    /// Tuya product ID.
    pub product_id: String,
}

/// A home/group containing devices.
#[derive(Debug, Clone)]
pub struct Home {
    /// Group/home ID.
    pub gid: u64,
    /// Home display name.
    pub name: String,
}

/// Cloud storage credentials for map download (AWS STS temporary).
#[derive(Debug, Clone)]
pub struct StorageCredentials {
    /// AWS access key ID.
    pub ak: String,
    /// AWS secret access key.
    pub sk: String,
    /// AWS session token.
    pub token: String,
    /// S3 bucket name.
    pub bucket: String,
    /// S3 region.
    pub region: String,
    /// Credentials expiration timestamp.
    pub expiration: String,
    /// S3 object key prefix for map files.
    pub path_prefix: String,
}

/// Build request parameters for a Tuya API call.
///
/// Returns pairs of (key, value). The `sign` parameter is computed and included.
pub fn build_request_params(
    creds: &OemCredentials,
    action: &str,
    version: &str,
    post_data: &str,
    session: Option<&Session>,
    timestamp: &str,
    request_id: &str,
) -> Vec<(String, String)> {
    let mut params: Vec<(&str, String)> = vec![
        ("a", action.to_string()),
        ("v", version.to_string()),
        ("clientId", creds.client_id.clone()),
        ("deviceId", creds.app_device_id.clone()),
        ("os", "Android".to_string()),
        ("lang", "en_US".to_string()),
        ("appVersion", "1.0.10".to_string()),
        ("ttid", format!("sdk_thing@{}", creds.client_id)),
        ("time", timestamp.to_string()),
        ("requestId", request_id.to_string()),
        ("chKey", "71c35f83".to_string()),
        ("postData", post_data.to_string()),
    ];
    if let Some(sess) = session {
        params.push(("sid", sess.sid.clone()));
    }

    let sign_pairs: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let sign_string = signing::build_sign_string(&sign_pairs);
    let hmac_key = creds.hmac_key();
    let sign = signing::compute_sign(&sign_string, &hmac_key);

    let mut result: Vec<(String, String)> = params
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    result.push(("sign".to_string(), sign));
    result
}

// ── AWS4 presigned URL signing ─────────────────────────────

/// Derive the AWS4 signing key: HMAC chain of date → region → service → "aws4_request".
pub fn derive_aws4_signing_key(
    secret_key: &str,
    date_stamp: &str,
    region: &str,
    service: &str,
) -> Vec<u8> {
    let k_date = hmac_sha256(
        format!("AWS4{secret_key}").as_bytes(),
        date_stamp.as_bytes(),
    );
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    hmac_sha256(&k_service, b"aws4_request")
}

/// Generate an AWS4-HMAC-SHA256 pre-signed URL.
///
/// # Examples
///
/// ```
/// use tuya_rs::api::generate_presigned_url;
///
/// let url = generate_presigned_url(
///     "/maps/lay.bin",
///     "AKIAIOSFODNN7EXAMPLE",
///     "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
///     "session-token",
///     "my-bucket.s3.amazonaws.com",
///     "eu-west-1",
///     "20260101T120000Z",
///     3600,
/// );
/// assert!(url.starts_with("https://"));
/// assert!(url.contains("X-Amz-Signature="));
/// ```
#[allow(clippy::too_many_arguments)]
pub fn generate_presigned_url(
    path: &str,
    ak: &str,
    sk: &str,
    token: &str,
    bucket: &str,
    region: &str,
    amz_date: &str,
    expires: u32,
) -> String {
    let date_stamp = &amz_date[..8];
    let host = format!("{bucket}.{region}");
    let credential_scope = format!("{date_stamp}/{region}/s3/aws4_request");
    let credential = format!("{ak}/{credential_scope}");

    // Canonical query string (sorted)
    let mut query_params = [
        ("X-Amz-Algorithm", "AWS4-HMAC-SHA256".to_string()),
        ("X-Amz-Credential", credential),
        ("X-Amz-Date", amz_date.to_string()),
        ("X-Amz-Expires", expires.to_string()),
        ("X-Amz-Security-Token", token.to_string()),
        ("X-Amz-SignedHeaders", "host".to_string()),
    ];
    query_params.sort_by_key(|(k, _)| *k);

    let canonical_querystring: String = query_params
        .iter()
        .map(|(k, v)| format!("{}={}", url_encode(k), url_encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    // Canonical request
    let canonical_request =
        format!("GET\n{path}\n{canonical_querystring}\nhost:{host}\n\nhost\nUNSIGNED-PAYLOAD");

    // String to sign
    let canonical_hash = sha256_hex(canonical_request.as_bytes());
    let string_to_sign =
        format!("AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{canonical_hash}");

    // Signature
    let signing_key = derive_aws4_signing_key(sk, date_stamp, region, "s3");
    let signature =
        crate::crypto::hex_encode(&hmac_sha256(&signing_key, string_to_sign.as_bytes()));

    format!("https://{host}{path}?{canonical_querystring}&X-Amz-Signature={signature}")
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC-SHA256 accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    crate::crypto::hex_encode(&hasher.finalize())
}

fn url_encode(s: &str) -> String {
    let mut result = String::new();
    for byte in s.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(*byte as char);
            }
            _ => {
                result.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    result
}

// ── HTTP client abstraction ────────────────────────────────

/// HTTP transport abstraction for Tuya API calls.
///
/// Implement this trait to provide a custom HTTP backend (e.g. for testing).
#[allow(async_fn_in_trait)]
pub trait HttpClient {
    /// Send a POST request with form parameters, returning the response body.
    async fn post_form(
        &self,
        endpoint: &str,
        params: &[(String, String)],
    ) -> Result<String, ApiError>;
}

/// Default HTTP client using [`reqwest`].
#[cfg(feature = "cloud")]
pub struct ReqwestClient {
    client: reqwest::Client,
}

#[cfg(feature = "cloud")]
impl HttpClient for ReqwestClient {
    async fn post_form(
        &self,
        endpoint: &str,
        params: &[(String, String)],
    ) -> Result<String, ApiError> {
        let url = reqwest::Url::parse_with_params(endpoint, params)
            .map_err(|e| ApiError::NetworkError(e.to_string()))?;
        let resp = self
            .client
            .post(url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .send()
            .await
            .map_err(|e| ApiError::NetworkError(e.to_string()))?;
        resp.text()
            .await
            .map_err(|e| ApiError::NetworkError(e.to_string()))
    }
}

// ── TuyaApi trait ──────────────────────────────────────────

#[allow(async_fn_in_trait)]
/// Tuya OEM Mobile API client trait.
pub trait TuyaApi {
    /// Authenticate with email and password, returning an active session.
    async fn login(&mut self, email: &str, password: &str) -> Result<Session, ApiError>;
    /// Return the current session, if logged in.
    fn session(&self) -> Option<&Session>;
    /// List all homes/groups for the logged-in user.
    async fn list_homes(&self) -> Result<Vec<Home>, ApiError>;
    /// List all devices in the given home/group.
    async fn list_devices(&self, gid: u64) -> Result<Vec<DeviceInfo>, ApiError>;
    /// Get temporary AWS credentials for downloading map files.
    async fn storage_config(&self, dev_id: &str) -> Result<StorageCredentials, ApiError>;
}

// ── Concrete API client ────────────────────────────────────

/// Tuya OEM API client, generic over the HTTP transport.
///
/// Use [`TuyaOemApi::new`] for the default [`ReqwestClient`] backend,
/// or [`TuyaOemApi::with_http`] to inject a custom [`HttpClient`] (e.g. for testing).
#[cfg(feature = "cloud")]
pub struct TuyaOemApi<H: HttpClient = ReqwestClient> {
    /// OEM app credentials.
    pub credentials: OemCredentials,
    /// Active session after login.
    pub session: Option<Session>,
    /// HTTP transport.
    pub http: H,
    /// API endpoint URL.
    pub endpoint: String,
}

#[cfg(feature = "cloud")]
impl TuyaOemApi {
    /// Create a new API client with the given OEM credentials.
    pub fn new(credentials: OemCredentials) -> Self {
        Self {
            credentials,
            session: None,
            http: ReqwestClient {
                client: reqwest::Client::new(),
            },
            endpoint: "https://a1.tuyaeu.com/api.json".to_string(),
        }
    }
}

#[cfg(feature = "cloud")]
impl<H: HttpClient> TuyaOemApi<H> {
    /// Create a new API client with a custom HTTP transport.
    pub fn with_http(credentials: OemCredentials, http: H) -> Self {
        Self {
            credentials,
            session: None,
            http,
            endpoint: "https://a1.tuyaeu.com/api.json".to_string(),
        }
    }

    /// Execute a raw Tuya API call, returning the response body.
    ///
    /// Builds signed request parameters, sends via the HTTP transport,
    /// and checks for Tuya error codes in the response.
    pub async fn raw_call(
        &self,
        action: &str,
        version: &str,
        post_data: &str,
        extra_params: &[(&str, &str)],
    ) -> Result<String, ApiError> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock is before UNIX epoch")
            .as_secs()
            .to_string();
        let request_id = uuid::Uuid::new_v4().to_string();

        let mut params = build_request_params(
            &self.credentials,
            action,
            version,
            post_data,
            self.session.as_ref(),
            &timestamp,
            &request_id,
        );

        for (k, v) in extra_params {
            params.push((k.to_string(), v.to_string()));
        }

        let body = self.http.post_form(&self.endpoint, &params).await?;

        // Check for API errors
        check_api_error(&body)?;

        Ok(body)
    }
}

/// Check a Tuya API response body for error codes.
///
/// Returns `Ok(())` if no error is found, or the appropriate [`ApiError`].
#[cfg(feature = "cloud")]
fn check_api_error(body: &str) -> Result<(), ApiError> {
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(body)
        && let Some(err_code) = json.get("errorCode").and_then(|v| v.as_str())
    {
        let msg = json
            .get("errorMsg")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        return match err_code {
            "USER_SESSION_INVALID" => Err(ApiError::SessionInvalid),
            "USER_PASSWD_WRONG" => Err(ApiError::PasswordWrong),
            "ILLEGAL_ACCESS_API" => Err(ApiError::IllegalAccessApi),
            _ => Err(ApiError::ServerError {
                code: err_code.to_string(),
                message: msg,
            }),
        };
    }
    Ok(())
}

#[cfg(feature = "cloud")]
impl<H: HttpClient> TuyaApi for TuyaOemApi<H> {
    async fn login(&mut self, email: &str, password: &str) -> Result<Session, ApiError> {
        use crate::crypto;
        use num_bigint::BigUint;

        // Step 1: token create
        let post_data = serde_json::json!({
            "countryCode": "",
            "email": email
        })
        .to_string();

        let resp = self
            .raw_call("tuya.m.user.email.token.create", "1.0", &post_data, &[])
            .await?;
        let resp: serde_json::Value =
            serde_json::from_str(&resp).map_err(|e| ApiError::ParseError(e.to_string()))?;

        let result = resp
            .get("result")
            .ok_or_else(|| ApiError::ParseError("no result in token response".into()))?;

        let token = result["token"]
            .as_str()
            .ok_or_else(|| ApiError::ParseError("no token".into()))?;
        let public_key = result["publicKey"]
            .as_str()
            .ok_or_else(|| ApiError::ParseError("no publicKey".into()))?;
        let exponent = result["exponent"]
            .as_str()
            .ok_or_else(|| ApiError::ParseError("no exponent".into()))?;

        // Step 2: encrypt password
        let modulus = public_key
            .parse::<BigUint>()
            .map_err(|e| ApiError::ParseError(format!("invalid publicKey: {e}")))?;
        let exp = exponent
            .parse::<BigUint>()
            .map_err(|e| ApiError::ParseError(format!("invalid exponent: {e}")))?;

        let encrypted_passwd = crypto::encrypt_password(password, &modulus, &exp);

        // Step 3: login
        let post_data = serde_json::json!({
            "countryCode": "",
            "email": email,
            "ifencrypt": 1,
            "options": "{\"group\": 1}",
            "passwd": encrypted_passwd,
            "token": token,
        })
        .to_string();

        let resp = self
            .raw_call("tuya.m.user.email.password.login", "1.0", &post_data, &[])
            .await?;
        let resp: serde_json::Value =
            serde_json::from_str(&resp).map_err(|e| ApiError::ParseError(e.to_string()))?;

        let result = resp.get("result").ok_or(ApiError::PasswordWrong)?;

        let session = Session {
            sid: result["sid"]
                .as_str()
                .ok_or_else(|| ApiError::ParseError("no sid".into()))?
                .to_string(),
            uid: result["uid"].as_str().unwrap_or("").to_string(),
            email: email.to_string(),
            domain: result
                .get("domain")
                .and_then(|d| d.get("mobileApiUrl"))
                .and_then(|v| v.as_str())
                .unwrap_or("https://a1.tuyaeu.com")
                .to_string(),
        };

        self.session = Some(session.clone());
        Ok(session)
    }

    fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    async fn list_homes(&self) -> Result<Vec<Home>, ApiError> {
        let resp = self
            .raw_call("tuya.m.location.list", "1.0", "{}", &[])
            .await?;
        let resp: serde_json::Value =
            serde_json::from_str(&resp).map_err(|e| ApiError::ParseError(e.to_string()))?;
        let results = resp["result"]
            .as_array()
            .ok_or_else(|| ApiError::ParseError("no result array".into()))?;

        Ok(results
            .iter()
            .filter_map(|h| {
                let gid = h
                    .get("groupId")
                    .or_else(|| h.get("gid"))
                    .and_then(|v| v.as_u64())?;
                let name = h["name"].as_str().unwrap_or("").to_string();
                Some(Home { gid, name })
            })
            .collect())
    }

    async fn list_devices(&self, gid: u64) -> Result<Vec<DeviceInfo>, ApiError> {
        let resp = self
            .raw_call(
                "tuya.m.my.group.device.list",
                "1.0",
                "{}",
                &[("gid", &gid.to_string())],
            )
            .await?;
        let resp: serde_json::Value =
            serde_json::from_str(&resp).map_err(|e| ApiError::ParseError(e.to_string()))?;
        let results = resp["result"]
            .as_array()
            .ok_or_else(|| ApiError::ParseError("no result array".into()))?;

        Ok(results
            .iter()
            .filter_map(|d| {
                Some(DeviceInfo {
                    dev_id: d["devId"].as_str()?.to_string(),
                    local_key: d["localKey"].as_str().unwrap_or("").to_string(),
                    name: d["name"].as_str().unwrap_or("").to_string(),
                    product_id: d["productId"].as_str().unwrap_or("").to_string(),
                })
            })
            .collect())
    }

    async fn storage_config(&self, dev_id: &str) -> Result<StorageCredentials, ApiError> {
        let post_data = serde_json::json!({
            "devId": dev_id,
            "type": "Common"
        })
        .to_string();

        let resp = self
            .raw_call("thing.m.dev.storage.config.get", "1.0", &post_data, &[])
            .await?;
        let resp: serde_json::Value =
            serde_json::from_str(&resp).map_err(|e| ApiError::ParseError(e.to_string()))?;
        let result = resp
            .get("result")
            .ok_or_else(|| ApiError::ParseError("no result".into()))?;

        Ok(StorageCredentials {
            ak: result["ak"].as_str().unwrap_or("").to_string(),
            sk: result["sk"].as_str().unwrap_or("").to_string(),
            token: result["token"].as_str().unwrap_or("").to_string(),
            bucket: result["bucket"]
                .as_str()
                .unwrap_or("ty-eu-storage-permanent")
                .to_string(),
            region: result["region"]
                .as_str()
                .unwrap_or("tuyaeu.com")
                .to_string(),
            expiration: result["expiration"].as_str().unwrap_or("").to_string(),
            path_prefix: result["pathConfig"]["common"]
                .as_str()
                .unwrap_or("")
                .to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_creds() -> OemCredentials {
        OemCredentials {
            client_id: "test_client_id_placeholder".into(),
            app_secret: "test_app_secret_placeholder_here".into(),
            bmp_key: "test_bmp_key_placeholder_here_xx".into(),
            cert_hash: "AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99".into(),
            package_name: "com.example.test.app".into(),
            app_device_id: "test_app_device_id_placeholder".into(),
        }
    }

    #[test]
    fn oem_credentials_hmac_key() {
        let creds = test_creds();
        let key = creds.hmac_key();
        // Format: packageName_certHash_bmpKey_appSecret
        assert!(key.starts_with("com.example.test.app_AA:BB:"));
        assert!(key.contains("_test_bmp_key_placeholder_here_xx_"));
        assert!(key.ends_with("_test_app_secret_placeholder_here"));
    }

    #[test]
    fn build_request_params_contains_required() {
        let creds = test_creds();
        let params = build_request_params(
            &creds,
            "tuya.m.location.list",
            "1.0",
            "{}",
            None,
            "1770808371",
            "test-uuid",
        );

        let find = |key: &str| -> Option<String> {
            params
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
        };

        assert_eq!(find("a").unwrap(), "tuya.m.location.list");
        assert_eq!(find("v").unwrap(), "1.0");
        assert_eq!(find("clientId").unwrap(), "test_client_id_placeholder");
        assert_eq!(find("os").unwrap(), "Android");
        assert_eq!(find("lang").unwrap(), "en_US");
        assert!(find("sign").is_some());
        assert!(find("postData").is_some());
        assert!(find("time").is_some());
        assert!(find("requestId").is_some());
        // No session → no sid
        assert!(find("sid").is_none());
    }

    #[test]
    fn build_request_params_with_session() {
        let creds = test_creds();
        let session = Session {
            sid: "test-sid".into(),
            uid: "uid".into(),
            email: "test@test.com".into(),
            domain: "https://a1.tuyaeu.com".into(),
        };
        let params =
            build_request_params(&creds, "test", "1.0", "{}", Some(&session), "123", "uuid");

        let find = |key: &str| -> Option<String> {
            params
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
        };

        assert_eq!(find("sid").unwrap(), "test-sid");
    }

    #[test]
    fn derive_aws4_signing_key_deterministic() {
        let key1 = derive_aws4_signing_key("mysecret", "20260213", "tuyaeu.com", "s3");
        let key2 = derive_aws4_signing_key("mysecret", "20260213", "tuyaeu.com", "s3");
        assert_eq!(key1, key2);
        assert_eq!(key1.len(), 32);

        // Different date → different key
        let key3 = derive_aws4_signing_key("mysecret", "20260214", "tuyaeu.com", "s3");
        assert_ne!(key1, key3);
    }

    #[test]
    fn generate_presigned_url_structure() {
        let url = generate_presigned_url(
            "/test/path/lay.bin",
            "TESTAKID",
            "testsecret",
            "testtoken",
            "ty-eu-storage-permanent",
            "tuyaeu.com",
            "20260213T120000Z",
            86400,
        );

        assert!(url.starts_with("https://ty-eu-storage-permanent.tuyaeu.com/test/path/lay.bin?"));
        assert!(url.contains("X-Amz-Algorithm=AWS4-HMAC-SHA256"));
        assert!(url.contains("X-Amz-Credential=TESTAKID"));
        assert!(url.contains("X-Amz-Date=20260213T120000Z"));
        assert!(url.contains("X-Amz-Expires=86400"));
        assert!(url.contains("X-Amz-Security-Token=testtoken"));
        assert!(url.contains("X-Amz-SignedHeaders=host"));
        assert!(url.contains("X-Amz-Signature="));
    }

    #[test]
    fn url_encode_special_chars() {
        assert_eq!(url_encode("hello world"), "hello%20world");
        assert_eq!(url_encode("a/b"), "a%2Fb");
        assert_eq!(url_encode("safe-chars_here.txt~"), "safe-chars_here.txt~");
    }

    #[test]
    fn url_encode_all_unreserved() {
        // All unreserved chars should pass through
        let unreserved = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.~";
        assert_eq!(url_encode(unreserved), unreserved);
    }

    #[test]
    fn url_encode_symbols() {
        assert_eq!(url_encode("+"), "%2B");
        assert_eq!(url_encode("="), "%3D");
        assert_eq!(url_encode("&"), "%26");
        assert_eq!(url_encode("@"), "%40");
        assert_eq!(url_encode(":"), "%3A");
    }

    #[test]
    fn sha256_hex_known_value() {
        // SHA-256 of empty string
        let hash = sha256_hex(b"");
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[cfg(feature = "cloud")]
    #[test]
    fn tuya_oem_api_new_defaults() {
        let creds = test_creds();
        let api = TuyaOemApi::new(creds.clone());
        assert!(api.session.is_none());
        assert_eq!(api.endpoint, "https://a1.tuyaeu.com/api.json");
        assert_eq!(api.credentials.client_id, creds.client_id);
    }

    // ── MockHttpClient + async tests (cloud feature) ──────

    #[cfg(feature = "cloud")]
    mod cloud_tests {
        use super::*;
        use std::cell::RefCell;
        use std::collections::VecDeque;

        struct MockHttpClient {
            responses: RefCell<VecDeque<String>>,
        }

        impl MockHttpClient {
            fn new(responses: Vec<&str>) -> Self {
                Self {
                    responses: RefCell::new(responses.into_iter().map(String::from).collect()),
                }
            }
        }

        impl HttpClient for MockHttpClient {
            async fn post_form(
                &self,
                _endpoint: &str,
                _params: &[(String, String)],
            ) -> Result<String, ApiError> {
                self.responses
                    .borrow_mut()
                    .pop_front()
                    .ok_or_else(|| ApiError::NetworkError("no more mock responses".into()))
            }
        }

        fn mock_api(responses: Vec<&str>) -> TuyaOemApi<MockHttpClient> {
            TuyaOemApi::with_http(test_creds(), MockHttpClient::new(responses))
        }

        // ── check_api_error ───────────────────────────────

        #[test]
        fn check_api_error_no_error() {
            assert!(check_api_error(r#"{"result":"ok"}"#).is_ok());
        }

        #[test]
        fn check_api_error_session_invalid() {
            let body = r#"{"errorCode":"USER_SESSION_INVALID","errorMsg":"session expired"}"#;
            assert!(matches!(
                check_api_error(body),
                Err(ApiError::SessionInvalid)
            ));
        }

        #[test]
        fn check_api_error_password_wrong() {
            let body = r#"{"errorCode":"USER_PASSWD_WRONG","errorMsg":"bad password"}"#;
            assert!(matches!(
                check_api_error(body),
                Err(ApiError::PasswordWrong)
            ));
        }

        #[test]
        fn check_api_error_illegal_access() {
            let body = r#"{"errorCode":"ILLEGAL_ACCESS_API","errorMsg":"denied"}"#;
            assert!(matches!(
                check_api_error(body),
                Err(ApiError::IllegalAccessApi)
            ));
        }

        #[test]
        fn check_api_error_unknown_code() {
            let body = r#"{"errorCode":"SOMETHING_ELSE","errorMsg":"oops"}"#;
            match check_api_error(body) {
                Err(ApiError::ServerError { code, message }) => {
                    assert_eq!(code, "SOMETHING_ELSE");
                    assert_eq!(message, "oops");
                }
                other => panic!("expected ServerError, got {other:?}"),
            }
        }

        #[test]
        fn check_api_error_no_error_msg() {
            let body = r#"{"errorCode":"CUSTOM_ERR"}"#;
            match check_api_error(body) {
                Err(ApiError::ServerError { message, .. }) => assert_eq!(message, ""),
                other => panic!("expected ServerError, got {other:?}"),
            }
        }

        #[test]
        fn check_api_error_non_json() {
            // Non-JSON body should pass through (no error detected)
            assert!(check_api_error("not json at all").is_ok());
        }

        // ── raw_call ──────────────────────────────────────

        #[tokio::test]
        async fn raw_call_success() {
            let api = mock_api(vec![r#"{"result":"ok"}"#]);
            let body = api.raw_call("test.action", "1.0", "{}", &[]).await.unwrap();
            assert_eq!(body, r#"{"result":"ok"}"#);
        }

        #[tokio::test]
        async fn raw_call_with_extra_params() {
            let api = mock_api(vec![r#"{"result":"ok"}"#]);
            let body = api
                .raw_call("test", "1.0", "{}", &[("gid", "123")])
                .await
                .unwrap();
            assert_eq!(body, r#"{"result":"ok"}"#);
        }

        #[tokio::test]
        async fn raw_call_error_response() {
            let api = mock_api(vec![
                r#"{"errorCode":"USER_SESSION_INVALID","errorMsg":"expired"}"#,
            ]);
            let err = api.raw_call("test", "1.0", "{}", &[]).await.unwrap_err();
            assert!(matches!(err, ApiError::SessionInvalid));
        }

        // ── list_homes ────────────────────────────────────

        #[tokio::test]
        async fn list_homes_success() {
            let api = mock_api(vec![
                r#"{"result":[{"groupId":1,"name":"Home"},{"gid":2,"name":"Office"}]}"#,
            ]);
            let homes = api.list_homes().await.unwrap();
            assert_eq!(homes.len(), 2);
            assert_eq!(homes[0].gid, 1);
            assert_eq!(homes[0].name, "Home");
            assert_eq!(homes[1].gid, 2);
            assert_eq!(homes[1].name, "Office");
        }

        #[tokio::test]
        async fn list_homes_empty() {
            let api = mock_api(vec![r#"{"result":[]}"#]);
            let homes = api.list_homes().await.unwrap();
            assert!(homes.is_empty());
        }

        #[tokio::test]
        async fn list_homes_no_result() {
            let api = mock_api(vec![r#"{"status":"ok"}"#]);
            let err = api.list_homes().await.unwrap_err();
            assert!(matches!(err, ApiError::ParseError(_)));
        }

        #[tokio::test]
        async fn list_homes_skips_invalid_entries() {
            // Entry without groupId/gid is skipped
            let api = mock_api(vec![
                r#"{"result":[{"name":"NoId"},{"groupId":5,"name":"Valid"}]}"#,
            ]);
            let homes = api.list_homes().await.unwrap();
            assert_eq!(homes.len(), 1);
            assert_eq!(homes[0].gid, 5);
        }

        // ── list_devices ──────────────────────────────────

        #[tokio::test]
        async fn list_devices_success() {
            let api = mock_api(vec![
                r#"{"result":[{"devId":"d1","localKey":"k1","name":"Robot","productId":"p1"}]}"#,
            ]);
            let devs = api.list_devices(1).await.unwrap();
            assert_eq!(devs.len(), 1);
            assert_eq!(devs[0].dev_id, "d1");
            assert_eq!(devs[0].local_key, "k1");
            assert_eq!(devs[0].name, "Robot");
            assert_eq!(devs[0].product_id, "p1");
        }

        #[tokio::test]
        async fn list_devices_defaults_for_missing_fields() {
            let api = mock_api(vec![r#"{"result":[{"devId":"d2"}]}"#]);
            let devs = api.list_devices(1).await.unwrap();
            assert_eq!(devs[0].local_key, "");
            assert_eq!(devs[0].name, "");
            assert_eq!(devs[0].product_id, "");
        }

        #[tokio::test]
        async fn list_devices_skips_without_dev_id() {
            let api = mock_api(vec![r#"{"result":[{"name":"NoId"},{"devId":"d3"}]}"#]);
            let devs = api.list_devices(1).await.unwrap();
            assert_eq!(devs.len(), 1);
            assert_eq!(devs[0].dev_id, "d3");
        }

        #[tokio::test]
        async fn list_devices_no_result() {
            let api = mock_api(vec![r#"{}"#]);
            assert!(matches!(
                api.list_devices(1).await.unwrap_err(),
                ApiError::ParseError(_)
            ));
        }

        // ── storage_config ────────────────────────────────

        #[tokio::test]
        async fn storage_config_success() {
            let api = mock_api(vec![
                r#"{"result":{
                "ak":"AK1","sk":"SK1","token":"TOK1",
                "bucket":"my-bucket","region":"eu-west-1",
                "expiration":"2026-01-01",
                "pathConfig":{"common":"/maps/dev1/"}
            }}"#,
            ]);
            let creds = api.storage_config("dev1").await.unwrap();
            assert_eq!(creds.ak, "AK1");
            assert_eq!(creds.sk, "SK1");
            assert_eq!(creds.token, "TOK1");
            assert_eq!(creds.bucket, "my-bucket");
            assert_eq!(creds.region, "eu-west-1");
            assert_eq!(creds.expiration, "2026-01-01");
            assert_eq!(creds.path_prefix, "/maps/dev1/");
        }

        #[tokio::test]
        async fn storage_config_defaults() {
            let api = mock_api(vec![r#"{"result":{}}"#]);
            let creds = api.storage_config("dev1").await.unwrap();
            assert_eq!(creds.ak, "");
            assert_eq!(creds.bucket, "ty-eu-storage-permanent");
            assert_eq!(creds.region, "tuyaeu.com");
            assert_eq!(creds.path_prefix, "");
        }

        #[tokio::test]
        async fn storage_config_no_result() {
            let api = mock_api(vec![r#"{}"#]);
            assert!(matches!(
                api.storage_config("dev1").await.unwrap_err(),
                ApiError::ParseError(_)
            ));
        }

        // ── login ─────────────────────────────────────────

        #[tokio::test]
        async fn login_success() {
            let mut api = mock_api(vec![
                // Step 1: token create response
                r#"{"result":{"token":"tok123","publicKey":"12345","exponent":"65537"}}"#,
                // Step 2: login response
                r#"{"result":{"sid":"session1","uid":"user1","domain":{"mobileApiUrl":"https://a2.tuyaeu.com"}}}"#,
            ]);
            let session = api.login("test@test.com", "password123").await.unwrap();
            assert_eq!(session.sid, "session1");
            assert_eq!(session.uid, "user1");
            assert_eq!(session.email, "test@test.com");
            assert_eq!(session.domain, "https://a2.tuyaeu.com");
            // Session is stored
            assert!(api.session().is_some());
            assert_eq!(api.session().unwrap().sid, "session1");
        }

        #[tokio::test]
        async fn login_default_domain() {
            let mut api = mock_api(vec![
                r#"{"result":{"token":"tok","publicKey":"12345","exponent":"65537"}}"#,
                r#"{"result":{"sid":"s1"}}"#,
            ]);
            let session = api.login("a@b.com", "pw").await.unwrap();
            assert_eq!(session.domain, "https://a1.tuyaeu.com");
        }

        #[tokio::test]
        async fn login_token_error_propagates() {
            let mut api = mock_api(vec![
                r#"{"errorCode":"ILLEGAL_ACCESS_API","errorMsg":"denied"}"#,
            ]);
            assert!(matches!(
                api.login("a@b.com", "pw").await.unwrap_err(),
                ApiError::IllegalAccessApi
            ));
        }

        #[tokio::test]
        async fn login_no_token_in_response() {
            let mut api = mock_api(vec![r#"{"result":{}}"#]);
            assert!(matches!(
                api.login("a@b.com", "pw").await.unwrap_err(),
                ApiError::ParseError(_)
            ));
        }

        #[tokio::test]
        async fn login_no_result_in_login_response() {
            let mut api = mock_api(vec![
                r#"{"result":{"token":"tok","publicKey":"12345","exponent":"65537"}}"#,
                r#"{"status":"error"}"#, // no "result" key
            ]);
            assert!(matches!(
                api.login("a@b.com", "pw").await.unwrap_err(),
                ApiError::PasswordWrong
            ));
        }

        #[tokio::test]
        async fn login_invalid_public_key() {
            let mut api = mock_api(vec![
                r#"{"result":{"token":"tok","publicKey":"not_a_number","exponent":"65537"}}"#,
            ]);
            assert!(matches!(
                api.login("a@b.com", "pw").await.unwrap_err(),
                ApiError::ParseError(_)
            ));
        }

        #[tokio::test]
        async fn login_no_sid_in_response() {
            let mut api = mock_api(vec![
                r#"{"result":{"token":"tok","publicKey":"12345","exponent":"65537"}}"#,
                r#"{"result":{"uid":"u1"}}"#, // no sid
            ]);
            assert!(matches!(
                api.login("a@b.com", "pw").await.unwrap_err(),
                ApiError::ParseError(_)
            ));
        }

        // ── session / with_http ───────────────────────────

        #[test]
        fn session_none_by_default() {
            let api = mock_api(vec![]);
            assert!(api.session().is_none());
        }

        #[test]
        fn with_http_sets_defaults() {
            let api = mock_api(vec![]);
            assert_eq!(api.endpoint, "https://a1.tuyaeu.com/api.json");
            assert!(api.session.is_none());
        }
    }
}
