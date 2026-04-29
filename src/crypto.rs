use aes_gcm::{
    aead::{rand_core::RngCore, Aead, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};

fn get_key() -> Result<Key<Aes256Gcm>> {
    let hex = std::env::var("TENANT_ENCRYPTION_KEY").context("TENANT_ENCRYPTION_KEY manquante")?;
    let bytes = hex::decode(&hex).context("Clé hex invalide")?;

    if bytes.len() != 32 {
        anyhow::bail!("TENANT_ENCRYPTION_KEY invalide: attendu 32 bytes, soit 64 caractères hex");
    }

    Ok(*Key::<Aes256Gcm>::from_slice(&bytes))
}

pub fn encrypt(plaintext: &str) -> Result<String> {
    let key = get_key()?;
    let cipher = Aes256Gcm::new(&key);

    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| anyhow::anyhow!("Chiffrement échoué: {}", e))?;

    let mut combined = nonce_bytes.to_vec();
    combined.extend(ciphertext);
    Ok(B64.encode(combined))
}

pub fn decrypt(encoded: &str) -> Result<String> {
    let key = get_key()?;
    let cipher = Aes256Gcm::new(&key);

    let combined = B64.decode(encoded).context("Base64 invalide")?;
    if combined.len() < 12 {
        anyhow::bail!("Payload chiffré invalide: nonce AES-GCM manquant");
    }

    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("Déchiffrement échoué: {}", e))?;

    String::from_utf8(plaintext).context("UTF-8 invalide")
}
