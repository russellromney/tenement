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
        Err(e) => {
            tracing::debug!("Invalid password hash format: {}", e);
            return false;
        }
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
    use std::collections::HashSet;

    // ===================
    // TOKEN GENERATION TESTS
    // ===================

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
    fn test_generate_token_uniqueness() {
        let mut tokens = HashSet::new();

        // Generate 100 tokens, all should be unique
        for _ in 0..100 {
            let token = generate_token();
            assert!(tokens.insert(token), "Token collision detected!");
        }

        assert_eq!(tokens.len(), 100);
    }

    #[test]
    fn test_generate_token_url_safe() {
        // Generate many tokens to ensure none have unsafe chars
        for _ in 0..50 {
            let token = generate_token();

            // URL-safe base64 shouldn't contain these
            assert!(!token.contains('+'), "Token contains +");
            assert!(!token.contains('/'), "Token contains /");
            assert!(!token.contains('='), "Token contains =");

            // Should only contain URL-safe chars
            assert!(
                token.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
                "Token contains non-URL-safe char: {}",
                token
            );
        }
    }

    #[test]
    fn test_generate_token_length() {
        // 32 bytes in URL-safe base64 without padding = 43 chars
        let token = generate_token();
        assert_eq!(token.len(), 43);
    }

    #[test]
    fn test_generate_token_entropy() {
        // Tokens should have high entropy (no repeated patterns)
        let tokens: Vec<String> = (0..10).map(|_| generate_token()).collect();

        for i in 0..tokens.len() {
            for j in (i + 1)..tokens.len() {
                // First 10 characters should differ
                assert_ne!(
                    &tokens[i][..10],
                    &tokens[j][..10],
                    "Tokens share common prefix"
                );
            }
        }
    }

    // ===================
    // HASH TESTS
    // ===================

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
    fn test_hash_produces_different_hashes() {
        // Argon2 uses random salt, so same input produces different hash
        let token = "test_token";
        let hash1 = hash_token(token).unwrap();
        let hash2 = hash_token(token).unwrap();

        // Hashes should be different (different salts)
        assert_ne!(hash1, hash2);

        // But both should verify
        assert!(verify_token(token, &hash1));
        assert!(verify_token(token, &hash2));
    }

    #[test]
    fn test_hash_format_argon2() {
        let token = generate_token();
        let hash = hash_token(&token).unwrap();

        // Should be Argon2 format
        assert!(hash.starts_with("$argon2"));
    }

    #[test]
    fn test_verify_invalid_hash() {
        let token = generate_token();

        // Invalid hash format should return false, not panic
        assert!(!verify_token(&token, "invalid_hash"));
        assert!(!verify_token(&token, ""));
    }

    #[test]
    fn test_verify_malformed_hashes() {
        let token = generate_token();

        let malformed = [
            "$argon2",
            "$argon2id$",
            "$argon2id$v=19$",
            "not_a_hash",
            "   ",
            "\n\n\n",
        ];

        for bad_hash in malformed {
            assert!(!verify_token(&token, bad_hash), "Should reject: {}", bad_hash);
        }
    }

    #[test]
    fn test_hash_empty_string() {
        // Should handle empty string
        let hash = hash_token("").unwrap();
        assert!(verify_token("", &hash));
        assert!(!verify_token("not_empty", &hash));
    }

    #[test]
    fn test_hash_long_token() {
        let long_token = "x".repeat(1000);
        let hash = hash_token(&long_token).unwrap();
        assert!(verify_token(&long_token, &hash));
    }

    #[test]
    fn test_hash_unicode() {
        let unicode = "token_🔐_émojis_字符";
        let hash = hash_token(unicode).unwrap();
        assert!(verify_token(unicode, &hash));
    }

    #[test]
    fn test_verify_case_sensitive() {
        let token = "MyToken123";
        let hash = hash_token(token).unwrap();

        assert!(verify_token("MyToken123", &hash));
        assert!(!verify_token("mytoken123", &hash));
        assert!(!verify_token("MYTOKEN123", &hash));
    }

    // ===================
    // TOKEN STORE TESTS
    // ===================

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

    #[tokio::test]
    async fn test_token_store_set_token() {
        use crate::store::{init_db, ConfigStore};
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");
        let pool = init_db(&path).await.unwrap();
        let config = ConfigStore::new(pool);
        let store = TokenStore::new(&config);

        // Set a specific token
        let my_token = "my_custom_token_12345";
        store.set_token(my_token).await.unwrap();

        assert!(store.has_token().await.unwrap());
        assert!(store.verify(my_token).await.unwrap());
        assert!(!store.verify("wrong_token").await.unwrap());
    }

    #[tokio::test]
    async fn test_token_store_replace_token() {
        use crate::store::{init_db, ConfigStore};
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");
        let pool = init_db(&path).await.unwrap();
        let config = ConfigStore::new(pool);
        let store = TokenStore::new(&config);

        // Set first token
        let token1 = store.generate_and_store().await.unwrap();
        assert!(store.verify(&token1).await.unwrap());

        // Replace with new token
        let token2 = store.generate_and_store().await.unwrap();

        // Old token should no longer work
        assert!(!store.verify(&token1).await.unwrap());

        // New token should work
        assert!(store.verify(&token2).await.unwrap());
    }

    #[tokio::test]
    async fn test_token_store_verify_no_token() {
        use crate::store::{init_db, ConfigStore};
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");
        let pool = init_db(&path).await.unwrap();
        let config = ConfigStore::new(pool);
        let store = TokenStore::new(&config);

        // No token set, verify should return false
        assert!(!store.verify("any_token").await.unwrap());
    }

    #[tokio::test]
    async fn test_token_store_clear_idempotent() {
        use crate::store::{init_db, ConfigStore};
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");
        let pool = init_db(&path).await.unwrap();
        let config = ConfigStore::new(pool);
        let store = TokenStore::new(&config);

        // Clear when no token exists should succeed
        store.clear().await.unwrap();
        assert!(!store.has_token().await.unwrap());

        // Clear again should still succeed
        store.clear().await.unwrap();
        assert!(!store.has_token().await.unwrap());
    }

    #[tokio::test]
    async fn test_generate_and_store_returns_unique() {
        use crate::store::{init_db, ConfigStore};
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");
        let pool = init_db(&path).await.unwrap();
        let config = ConfigStore::new(pool);
        let store = TokenStore::new(&config);

        let token1 = store.generate_and_store().await.unwrap();
        let token2 = store.generate_and_store().await.unwrap();

        // Each call should generate a unique token
        assert_ne!(token1, token2);
    }
}
