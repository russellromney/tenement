//! Authentication module
//!
//! Provides Bearer token authentication for the API.

use anyhow::Result;
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::Rng;

/// Length of generated tokens in bytes (32 bytes = 256 bits)
const TOKEN_LENGTH: usize = 32;

/// Generate a new random token
pub fn generate_token() -> String {
    let mut bytes = [0u8; TOKEN_LENGTH];
    rand::thread_rng().fill(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Hash a token for storage
pub fn hash_token(token: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(token.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("Failed to hash token: {}", e))?;
    Ok(hash.to_string())
}

/// Verify a token against a stored hash
pub fn verify_token(token: &str, hash: &str) -> bool {
    let parsed_hash = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(token.as_bytes(), &parsed_hash)
        .is_ok()
}

/// Token storage interface using ConfigStore
pub struct TokenStore<'a> {
    config: &'a crate::store::ConfigStore,
}

impl<'a> TokenStore<'a> {
    const TOKEN_HASH_KEY: &'static str = "api_token_hash";

    pub fn new(config: &'a crate::store::ConfigStore) -> Self {
        Self { config }
    }

    /// Check if a token is set
    pub async fn has_token(&self) -> Result<bool> {
        Ok(self.config.get(Self::TOKEN_HASH_KEY).await?.is_some())
    }

    /// Set a new token (stores the hash)
    pub async fn set_token(&self, token: &str) -> Result<()> {
        let hash = hash_token(token)?;
        self.config.set(Self::TOKEN_HASH_KEY, &hash).await
    }

    /// Verify a token
    pub async fn verify(&self, token: &str) -> Result<bool> {
        match self.config.get(Self::TOKEN_HASH_KEY).await? {
            Some(hash) => Ok(verify_token(token, &hash)),
            None => Ok(false),
        }
    }

    /// Generate and store a new token, returning the plaintext
    pub async fn generate_and_store(&self) -> Result<String> {
        let token = generate_token();
        self.set_token(&token).await?;
        Ok(token)
    }

    /// Clear the token
    pub async fn clear(&self) -> Result<()> {
        self.config.delete(Self::TOKEN_HASH_KEY).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_token() {
        let token1 = generate_token();
        let token2 = generate_token();

        // Should be URL-safe base64, ~43 chars for 32 bytes
        assert!(token1.len() >= 40);
        assert!(token1.len() <= 50);

        // Tokens should be unique
        assert_ne!(token1, token2);

        // Should be URL-safe (no +, /, =)
        assert!(!token1.contains('+'));
        assert!(!token1.contains('/'));
    }

    #[test]
    fn test_hash_and_verify() {
        let token = generate_token();
        let hash = hash_token(&token).unwrap();

        // Hash should be different from token
        assert_ne!(token, hash);

        // Should verify correctly
        assert!(verify_token(&token, &hash));

        // Wrong token should not verify
        let wrong_token = generate_token();
        assert!(!verify_token(&wrong_token, &hash));
    }

    #[test]
    fn test_verify_invalid_hash() {
        let token = generate_token();

        // Invalid hash format should return false, not panic
        assert!(!verify_token(&token, "invalid_hash"));
        assert!(!verify_token(&token, ""));
    }

    #[tokio::test]
    async fn test_token_store() {
        use crate::store::{init_db, ConfigStore};
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");
        let pool = init_db(&path).await.unwrap();
        let config = ConfigStore::new(pool);
        let store = TokenStore::new(&config);

        // Initially no token
        assert!(!store.has_token().await.unwrap());

        // Generate and store
        let token = store.generate_and_store().await.unwrap();
        assert!(store.has_token().await.unwrap());

        // Verify correct token
        assert!(store.verify(&token).await.unwrap());

        // Verify wrong token
        let wrong = generate_token();
        assert!(!store.verify(&wrong).await.unwrap());

        // Clear token
        store.clear().await.unwrap();
        assert!(!store.has_token().await.unwrap());
    }
}
