use hmac::{Hmac, Mac};
use sha1::Sha1;
use sha2::Sha256;
use thiserror::Error;

type HmacSha256 = Hmac<Sha256>;
type HmacSha1 = Hmac<Sha1>;

pub fn verify_github(
    secret: &[u8],
    body: &[u8],
    signature_header: &str,
) -> Result<(), SignatureError> {
    let encoded = signature_header
        .strip_prefix("sha256=")
        .ok_or(SignatureError::InvalidFormat)?;
    let signature = decode_signature(encoded)?;
    let mut mac = HmacSha256::new_from_slice(secret).map_err(|_| SignatureError::InvalidSecret)?;
    mac.update(body);
    mac.verify_slice(&signature)
        .map_err(|_| SignatureError::Mismatch)
}

pub fn verify_vercel(
    secret: &[u8],
    body: &[u8],
    signature_header: &str,
) -> Result<(), SignatureError> {
    let signature = decode_signature(signature_header)?;
    let mut mac = HmacSha1::new_from_slice(secret).map_err(|_| SignatureError::InvalidSecret)?;
    mac.update(body);
    mac.verify_slice(&signature)
        .map_err(|_| SignatureError::Mismatch)
}

fn decode_signature(encoded: &str) -> Result<Vec<u8>, SignatureError> {
    if encoded.is_empty() || !encoded.len().is_multiple_of(2) {
        return Err(SignatureError::InvalidFormat);
    }
    hex::decode(encoded).map_err(|_| SignatureError::InvalidFormat)
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SignatureError {
    #[error("signature format is invalid")]
    InvalidFormat,
    #[error("webhook secret is invalid")]
    InvalidSecret,
    #[error("webhook signature does not match")]
    Mismatch,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_githubs_published_test_vector() {
        let secret = b"It's a Secret to Everybody";
        let body = b"Hello, World!";
        let signature = "sha256=757107ea0eb2509fc211221cce984b8a37570b6d7586c22c46f4379c8b043e17";
        assert_eq!(verify_github(secret, body, signature), Ok(()));
    }

    #[test]
    fn rejects_tampered_github_body() {
        let signature = "sha256=757107ea0eb2509fc211221cce984b8a37570b6d7586c22c46f4379c8b043e17";
        assert_eq!(
            verify_github(
                b"It's a Secret to Everybody",
                b"Hello, attacker!",
                signature
            ),
            Err(SignatureError::Mismatch)
        );
    }

    #[test]
    fn validates_vercel_hmac_sha1() {
        let mut mac = HmacSha1::new_from_slice(b"vercel-secret").unwrap();
        mac.update(br#"{"type":"deployment.ready"}"#);
        let signature = hex::encode(mac.finalize().into_bytes());
        assert_eq!(
            verify_vercel(
                b"vercel-secret",
                br#"{"type":"deployment.ready"}"#,
                &signature
            ),
            Ok(())
        );
    }
}
