use anyhow::{Context, Result, anyhow, ensure};
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use hpke::{Deserializable, Kem, OpModeR, OpModeS, Serializable};
use rand::TryRngCore;
use ring::{
    aead, digest,
    rand::{SecureRandom, SystemRandom},
    signature::{self, KeyPair},
};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

type K = hpke::kem::X25519HkdfSha256;
type A = hpke::aead::ChaCha20Poly1305;
type D = hpke::kdf::HkdfSha256;

pub fn sha256(data: &[u8]) -> String {
    hex::encode(digest::digest(&digest::SHA256, data))
}
pub fn random<const N: usize>() -> Result<[u8; N]> {
    let mut data = [0u8; N];
    SystemRandom::new()
        .fill(&mut data)
        .map_err(|_| anyhow!("system RNG failed"))?;
    Ok(data)
}
pub fn random_id() -> Result<String> {
    Ok(hex::encode(random::<16>()?))
}
pub fn b64(bytes: &[u8]) -> String {
    B64.encode(bytes)
}
pub fn unb64(s: &str) -> Result<Vec<u8>> {
    Ok(B64.decode(s)?)
}
pub fn verify_p256(key: &[u8], data: &[u8], sig: &[u8]) -> Result<()> {
    signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, key)
        .verify(data, sig)
        .map_err(|_| anyhow!("invalid device proof"))
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ModuleManifest {
    pub protocol: u32,
    pub build_id: String,
    pub module_id: String,
    pub plaintext_sha256: String,
    pub ciphertext_sha256: String,
    pub ciphertext_bytes: u64,
    pub nonce: String,
    pub created_ms: i64,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct SignedManifest {
    pub manifest: ModuleManifest,
    pub signature: String,
}
pub fn sign_manifest(
    manifest: ModuleManifest,
    signing: &signature::Ed25519KeyPair,
) -> Result<SignedManifest> {
    let signature = b64(signing.sign(&serde_json::to_vec(&manifest)?).as_ref());
    Ok(SignedManifest {
        manifest,
        signature,
    })
}
pub fn verify_manifest(manifest: &SignedManifest, public_key: &[u8]) -> Result<()> {
    verify_server(
        public_key,
        &serde_json::to_vec(&manifest.manifest)?,
        &manifest.signature,
    )
}
pub fn verify_server(key: &[u8], data: &[u8], sig: &str) -> Result<()> {
    signature::UnparsedPublicKey::new(&signature::ED25519, key)
        .verify(data, &unb64(sig)?)
        .map_err(|_| anyhow!("invalid server signature"))
}
pub fn module_aad(build: &str, module: &str) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(&(
        "nova-module-aes256gcm-v1",
        build,
        module,
    ))?)
}
pub fn encrypt_module(
    plaintext: &[u8],
    key: &[u8],
    build: &str,
    module: &str,
) -> Result<(Vec<u8>, ModuleManifest)> {
    let nonce = random::<12>()?;
    let sealing = aead::LessSafeKey::new(
        aead::UnboundKey::new(&aead::AES_256_GCM, key)
            .map_err(|_| anyhow!("invalid content key"))?,
    );
    let mut ciphertext = plaintext.to_vec();
    sealing
        .seal_in_place_append_tag(
            aead::Nonce::assume_unique_for_key(nonce),
            aead::Aad::from(module_aad(build, module)?),
            &mut ciphertext,
        )
        .map_err(|_| anyhow!("module seal failed"))?;
    let manifest = ModuleManifest {
        protocol: 1,
        build_id: build.into(),
        module_id: module.into(),
        plaintext_sha256: sha256(plaintext),
        ciphertext_sha256: sha256(&ciphertext),
        ciphertext_bytes: ciphertext.len() as u64,
        nonce: b64(&nonce),
        created_ms: crate::unix_ms(),
    };
    Ok((ciphertext, manifest))
}
pub fn decrypt_module(
    ciphertext: &[u8],
    key: &[u8],
    manifest: &SignedManifest,
    public_key: &[u8],
) -> Result<Zeroizing<Vec<u8>>> {
    verify_manifest(manifest, public_key)?;
    let m = &manifest.manifest;
    ensure!(
        ciphertext.len() as u64 == m.ciphertext_bytes && sha256(ciphertext) == m.ciphertext_sha256,
        "module ciphertext identity mismatch"
    );
    let nonce: [u8; 12] = unb64(&m.nonce)?
        .try_into()
        .map_err(|_| anyhow!("invalid nonce"))?;
    let opening = aead::LessSafeKey::new(
        aead::UnboundKey::new(&aead::AES_256_GCM, key)
            .map_err(|_| anyhow!("invalid content key"))?,
    );
    let mut plaintext = Zeroizing::new(ciphertext.to_vec());
    let length = opening
        .open_in_place(
            aead::Nonce::assume_unique_for_key(nonce),
            aead::Aad::from(module_aad(&m.build_id, &m.module_id)?),
            &mut plaintext,
        )
        .map_err(|_| anyhow!("module authentication failed"))?
        .len();
    plaintext.truncate(length);
    ensure!(
        sha256(&plaintext) == m.plaintext_sha256,
        "module plaintext identity mismatch"
    );
    Ok(plaintext)
}

#[derive(Clone, Serialize, Deserialize)]
pub struct WrappedKey {
    pub encapsulated_key: String,
    pub ciphertext: String,
}
pub struct Recipient {
    secret: <K as Kem>::PrivateKey,
    pub public_key: String,
}
impl Recipient {
    pub fn new() -> Self {
        let (secret, public) = K::gen_keypair(&mut rand::rngs::OsRng.unwrap_err());
        Self {
            secret,
            public_key: b64(public.to_bytes().as_slice()),
        }
    }
    pub fn unwrap(&self, wrapped: &WrappedKey, binding: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        let enc = <K as Kem>::EncappedKey::from_bytes(&unb64(&wrapped.encapsulated_key)?)
            .map_err(|_| anyhow!("invalid HPKE encapsulation"))?;
        let mut receiver = hpke::setup_receiver::<A, D, K>(
            &OpModeR::Base,
            &self.secret,
            &enc,
            b"nova-task-content-key-v1",
        )
        .map_err(|_| anyhow!("HPKE receiver failed"))?;
        let plaintext = receiver
            .open(&unb64(&wrapped.ciphertext)?, binding)
            .map_err(|_| anyhow!("HPKE authentication failed"))?;
        ensure!(plaintext.len() == 32, "invalid content key length");
        Ok(Zeroizing::new(plaintext))
    }
}
pub fn wrap_key(public_key: &str, content_key: &[u8], binding: &[u8]) -> Result<WrappedKey> {
    let public = <K as Kem>::PublicKey::from_bytes(&unb64(public_key)?)
        .map_err(|_| anyhow!("invalid HPKE recipient"))?;
    let (enc, mut sender) = hpke::setup_sender::<A, D, K, _>(
        &OpModeS::Base,
        &public,
        b"nova-task-content-key-v1",
        &mut rand::rngs::OsRng.unwrap_err(),
    )
    .map_err(|_| anyhow!("HPKE sender failed"))?;
    let ciphertext = sender
        .seal(content_key, binding)
        .map_err(|_| anyhow!("HPKE wrapping failed"))?;
    Ok(WrappedKey {
        encapsulated_key: b64(enc.to_bytes().as_slice()),
        ciphertext: b64(&ciphertext),
    })
}
pub fn validate_recipient(public_key: &str) -> Result<()> {
    <K as Kem>::PublicKey::from_bytes(&unb64(public_key)?)
        .map_err(|_| anyhow!("invalid HPKE recipient"))?;
    Ok(())
}
pub fn load_signing_key(path: &std::path::Path) -> Result<signature::Ed25519KeyPair> {
    let bytes = Zeroizing::new(std::fs::read(path).context("read external signing key")?);
    signature::Ed25519KeyPair::from_pkcs8(&bytes).map_err(|_| anyhow!("invalid signing key"))
}
pub fn signing_public(key: &signature::Ed25519KeyPair) -> String {
    b64(key.public_key().as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authenticated_module_and_task_binding() {
        let signing_document =
            signature::Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        let signing = signature::Ed25519KeyPair::from_pkcs8(signing_document.as_ref()).unwrap();
        let key = random::<32>().unwrap();
        let (mut ciphertext, manifest) =
            encrypt_module(b"test module bytes only", &key, "build-test", "core").unwrap();
        let signed = sign_manifest(manifest, &signing).unwrap();
        assert_eq!(
            &*decrypt_module(&ciphertext, &key, &signed, signing.public_key().as_ref()).unwrap(),
            b"test module bytes only"
        );
        let recipient = Recipient::new();
        let wrapped = wrap_key(
            &recipient.public_key,
            &key,
            b"device/build/module/task/expiry",
        )
        .unwrap();
        assert_eq!(
            recipient
                .unwrap(&wrapped, b"device/build/module/task/expiry")
                .unwrap()
                .as_slice(),
            key
        );
        assert!(recipient.unwrap(&wrapped, b"different task").is_err());
        ciphertext[0] ^= 1;
        assert!(decrypt_module(&ciphertext, &key, &signed, signing.public_key().as_ref()).is_err());
        let mut forged = signed.clone();
        forged.manifest.build_id = "wrong-build".into();
        assert!(verify_manifest(&forged, signing.public_key().as_ref()).is_err());
    }
}
