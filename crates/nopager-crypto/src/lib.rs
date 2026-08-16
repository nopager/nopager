use base64::{Engine as _, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit},
};
use rand::RngCore;
use secrecy::{ExposeSecret, SecretString};
use thiserror::Error;

const NONCE_LEN: usize = 24;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("master key must decode to exactly 32 bytes")]
    InvalidKey,
    #[error("encrypted secret is malformed")]
    MalformedCiphertext,
    #[error("secret encryption failed")]
    EncryptionFailed,
    #[error("secret decryption failed")]
    DecryptionFailed,
}

pub struct SecretCipher(XChaCha20Poly1305);

impl SecretCipher {
    pub fn from_base64_key(key: &SecretString) -> Result<Self, CryptoError> {
        let bytes = STANDARD
            .decode(key.expose_secret())
            .map_err(|_| CryptoError::InvalidKey)?;
        let key: [u8; 32] = bytes.try_into().map_err(|_| CryptoError::InvalidKey)?;
        Ok(Self(XChaCha20Poly1305::new((&key).into())))
    }

    pub fn encrypt(&self, plaintext: &SecretString) -> Result<String, CryptoError> {
        let mut nonce = [0_u8; NONCE_LEN];
        rand::rng().fill_bytes(&mut nonce);
        let ciphertext = self
            .0
            .encrypt(
                XNonce::from_slice(&nonce),
                plaintext.expose_secret().as_bytes(),
            )
            .map_err(|_| CryptoError::EncryptionFailed)?;
        let mut payload = nonce.to_vec();
        payload.extend(ciphertext);
        Ok(STANDARD.encode(payload))
    }

    pub fn decrypt(&self, encoded: &str) -> Result<SecretString, CryptoError> {
        let payload = STANDARD
            .decode(encoded)
            .map_err(|_| CryptoError::MalformedCiphertext)?;
        if payload.len() <= NONCE_LEN {
            return Err(CryptoError::MalformedCiphertext);
        }
        let (nonce, ciphertext) = payload.split_at(NONCE_LEN);
        let plaintext = self
            .0
            .decrypt(XNonce::from_slice(nonce), ciphertext)
            .map_err(|_| CryptoError::DecryptionFailed)?;
        String::from_utf8(plaintext)
            .map(SecretString::from)
            .map_err(|_| CryptoError::DecryptionFailed)
    }
}

#[must_use]
pub fn mask_secret(value: &str) -> String {
    let suffix: String = value
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if suffix.is_empty() {
        "••••••••".to_owned()
    } else {
        format!("••••••••{suffix}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypts_and_decrypts_without_exposing_plaintext() {
        let key = SecretString::from(STANDARD.encode([7_u8; 32]));
        let cipher = SecretCipher::from_base64_key(&key).unwrap();
        let secret = SecretString::from("sk-production-secret".to_owned());
        let encrypted = cipher.encrypt(&secret).unwrap();
        assert!(!encrypted.contains("production"));
        assert_eq!(
            cipher.decrypt(&encrypted).unwrap().expose_secret(),
            secret.expose_secret()
        );
    }

    #[test]
    fn masking_only_keeps_a_short_suffix() {
        assert_eq!(mask_secret("sk-abcdefgh1234"), "••••••••1234");
    }
}
