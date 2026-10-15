use serde::{Deserialize, Serialize};

pub const PROTOCOL: u32 = 1;
pub const CLOCK_SKEW_MS: i64 = 60_000;
pub const TASK_LIFETIME_MS: i64 = 24 * 60 * 60 * 1000;

#[derive(Clone, Serialize, Deserialize)]
pub struct Proof {
    pub device_id: String,
    pub request_id: String,
    pub unix_ms: i64,
    pub signature: String,
}

#[derive(Serialize)]
struct Canonical<'a> {
    protocol: u32,
    device_id: &'a str,
    request_id: &'a str,
    unix_ms: i64,
    method: &'a str,
    endpoint: &'a str,
    body_sha256: String,
}

pub fn canonical(proof: &Proof, endpoint: &str, body: &[u8]) -> Vec<u8> {
    serde_json::to_vec(&Canonical {
        protocol: PROTOCOL,
        device_id: &proof.device_id,
        request_id: &proof.request_id,
        unix_ms: proof.unix_ms,
        method: "POST",
        endpoint,
        body_sha256: crate::crypto::sha256(body),
    })
    .expect("serialize canonical request")
}

#[derive(Serialize, Deserialize)]
pub struct RegisterRequest {
    pub public_key: String,
    pub request_id: String,
    pub unix_ms: i64,
    pub signature: String,
}
pub fn registration_message(key: &str, id: &str, time: i64) -> Vec<u8> {
    serde_json::to_vec(&("nova-device-register-v1", key, id, time)).expect("registration")
}
#[derive(Serialize, Deserialize)]
pub struct RegisterResponse {
    pub device_id: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AcquireRequest {
    pub build_id: String,
    pub module_id: String,
    pub task_id: String,
    pub recipient_key: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskBinding {
    pub device_id: String,
    pub build_id: String,
    pub module_id: String,
    pub task_id: String,
    pub expires_ms: i64,
    pub ciphertext_sha256: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct AcquireResponse {
    pub binding: TaskBinding,
    pub task_token: String,
    pub wrapped_key: crate::crypto::WrappedKey,
    pub signed_manifest: crate::crypto::SignedManifest,
    pub server_signature: String,
}
impl std::fmt::Debug for AcquireResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcquireResponse")
            .field("binding", &self.binding)
            .field("secret_fields", &"redacted")
            .finish()
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
    pub unban_ms: Option<i64>,
    pub retry_after_seconds: Option<u64>,
}
pub fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.:".contains(&c))
}
