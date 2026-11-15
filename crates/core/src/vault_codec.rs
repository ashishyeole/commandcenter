use crate::{archive::VaultEntry, Result};
use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use rand::{rngs::OsRng, RngCore};
use zeroize::Zeroizing;
pub fn seal(key: &[u8], entry: &VaultEntry) -> Result<Vec<u8>> {
    entry.validate()?;
    let data = Zeroizing::new(serde_json::to_vec(entry).map_err(|e| e.to_string())?);
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| "Invalid vault key")?;
    let encrypted = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &data,
                aad: entry.id.as_bytes(),
            },
        )
        .map_err(|_| "Vault encryption failed")?;
    let mut bytes = nonce.to_vec();
    bytes.extend(encrypted);
    Ok(bytes)
}
pub fn reveal(key: &[u8], id: &str, bytes: &[u8]) -> Result<VaultEntry> {
    if bytes.len() < 28 {
        return Err("Invalid encrypted record".into());
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| "Invalid vault key")?;
    let data = Zeroizing::new(
        cipher
            .decrypt(
                Nonce::from_slice(&bytes[..12]),
                Payload {
                    msg: &bytes[12..],
                    aad: id.as_bytes(),
                },
            )
            .map_err(|_| "Vault authentication failed")?,
    );
    let entry: VaultEntry = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
    entry.validate()?;
    Ok(entry)
}

pub fn new_key() -> Zeroizing<Vec<u8>> {
    let mut key = Zeroizing::new(vec![0u8; 32]);
    OsRng.fill_bytes(&mut key);
    key
}
