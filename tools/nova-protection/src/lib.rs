//! Private task protection transport. No rendering algorithms live in this crate.
#[cfg(feature = "client")]
pub mod client;
pub mod crypto;
#[cfg(feature = "client")]
mod identity;
pub mod protocol;
pub mod runtime;

pub fn unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}
