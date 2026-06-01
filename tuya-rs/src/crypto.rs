use aes::Aes128;
use cipher::{BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};
use thiserror::Error;

/// Cryptographic operation errors.
#[derive(Debug, Error)]
pub enum CryptoError {
    /// PKCS7 padding is invalid.
    #[error("invalid padding")]
    InvalidPadding,
    /// Data length is not a multiple of 16.
    #[error("data length must be a multiple of 16 for raw decryption")]
    InvalidLength,
}

/// AES-128-ECB encrypt with PKCS7 padding.
///
/// # Examples
///
/// ```
/// use tuya_rs::crypto::{aes_ecb_encrypt, aes_ecb_decrypt};
///
/// let key = b"0123456789abcdef";
/// let plaintext = b"hello world";
/// let ciphertext = aes_ecb_encrypt(key, plaintext);
/// let decrypted = aes_ecb_decrypt(key, &ciphertext).unwrap();
/// assert_eq!(decrypted, plaintext);
/// ```
pub fn aes_ecb_encrypt(key: &[u8; 16], data: &[u8]) -> Vec<u8> {
    let cipher = Aes128::new(key.into());

    // PKCS7 padding
    let pad_len = 16 - (data.len() % 16);
    let mut padded = Vec::with_capacity(data.len() + pad_len);
    padded.extend_from_slice(data);
    padded.resize(data.len() + pad_len, pad_len as u8);

    // Encrypt all 16-byte blocks
    let (blocks, _) = cipher::Array::slice_as_chunks_mut(&mut padded);
    cipher.encrypt_blocks(blocks);

    padded
}

/// AES-128-ECB decrypt and remove PKCS7 padding.
///
/// # Examples
///
/// ```
/// use tuya_rs::crypto::{aes_ecb_encrypt, aes_ecb_decrypt};
///
/// let key = b"0123456789abcdef";
/// let ciphertext = aes_ecb_encrypt(key, b"secret data");
/// let plaintext = aes_ecb_decrypt(key, &ciphertext).unwrap();
/// assert_eq!(plaintext, b"secret data");
/// ```
pub fn aes_ecb_decrypt(key: &[u8; 16], data: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if !data.len().is_multiple_of(16) || data.is_empty() {
        return Err(CryptoError::InvalidLength);
    }

    let cipher = Aes128::new(key.into());
    let mut buf = data.to_vec();

    let (blocks, _) = cipher::Array::slice_as_chunks_mut(&mut buf);
    cipher.decrypt_blocks(blocks);

    // Remove PKCS7 padding
    // Safety: buf is non-empty (early return above rejects empty input)
    let &pad_byte = buf.last().ok_or(CryptoError::InvalidPadding)?;
    if pad_byte == 0 || pad_byte as usize > 16 {
        return Err(CryptoError::InvalidPadding);
    }
    let pad_len = pad_byte as usize;
    if buf.len() < pad_len {
        return Err(CryptoError::InvalidPadding);
    }
    if !buf[buf.len() - pad_len..].iter().all(|&b| b == pad_byte) {
        return Err(CryptoError::InvalidPadding);
    }
    buf.truncate(buf.len() - pad_len);
    Ok(buf)
}

/// Textbook RSA encryption (no padding): `msg^e mod n`.
///
/// Used by Tuya for password encryption during login.
#[cfg(feature = "cloud")]
pub fn rsa_encrypt_textbook(
    plaintext: &[u8],
    modulus: &num_bigint::BigUint,
    exponent: &num_bigint::BigUint,
) -> Vec<u8> {
    use num_bigint::BigUint;

    let msg = BigUint::from_bytes_be(plaintext);
    let encrypted = msg.modpow(exponent, modulus);
    let byte_len = (modulus.bits() as usize).div_ceil(8);
    let raw = encrypted.to_bytes_be();
    // Pad to modulus byte length
    let mut result = vec![0u8; byte_len.saturating_sub(raw.len())];
    result.extend_from_slice(&raw);
    result
}

/// Encrypt a password for Tuya login: MD5(password) → hex → RSA encrypt → hex output.
#[cfg(feature = "cloud")]
pub fn encrypt_password(
    password: &str,
    modulus: &num_bigint::BigUint,
    exponent: &num_bigint::BigUint,
) -> String {
    use md5::{Digest, Md5};

    let mut hasher = Md5::new();
    hasher.update(password.as_bytes());
    let md5_hex = format!("{:x}", hasher.finalize());

    let encrypted = rsa_encrypt_textbook(md5_hex.as_bytes(), modulus, exponent);
    hex_encode(&encrypted)
}

/// Encode bytes as lowercase hex string.
#[cfg(feature = "cloud")]
pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aes_ecb_roundtrip_non_aligned() {
        let key = b"0123456789abcdef";
        let plaintext = b"hello world";
        let encrypted = aes_ecb_encrypt(key, plaintext);
        assert_eq!(encrypted.len(), 16); // 11 + 5 padding
        let decrypted = aes_ecb_decrypt(key, &encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn aes_ecb_decrypt_invalid_length() {
        let key = b"0123456789abcdef";
        assert!(aes_ecb_decrypt(key, &[0u8; 15]).is_err());
        assert!(aes_ecb_decrypt(key, &[]).is_err());
    }

    #[cfg(feature = "cloud")]
    mod cloud {
        use super::*;
        use num_bigint::BigUint;

        #[test]
        fn rsa_textbook_small_values() {
            let n = BigUint::from(3233u32);
            let e = BigUint::from(17u32);
            let d = BigUint::from(2753u32);

            let plain = BigUint::from(65u32);
            let encrypted = plain.modpow(&e, &n);
            assert_eq!(encrypted, BigUint::from(2790u32));

            let decrypted = encrypted.modpow(&d, &n);
            assert_eq!(decrypted, plain);

            let result = rsa_encrypt_textbook(&[65], &n, &e);
            let result_int = BigUint::from_bytes_be(&result);
            assert_eq!(result_int, BigUint::from(2790u32));
        }

        #[test]
        fn password_format() {
            let n = BigUint::from(3233u32);
            let e = BigUint::from(17u32);
            let result = encrypt_password("password", &n, &e);
            assert!(result.chars().all(|c| c.is_ascii_hexdigit()));
        }
    }
}
