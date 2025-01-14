// Copyright 2019-2023 ChainSafe Systems
// SPDX-License-Identifier: Apache-2.0, MIT

use crate::key_management::KeyInfo;
use crate::shim::crypto::SignatureType;
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, errors::Result as JWTResult, DecodingKey, EncodingKey, Header};
use rand::Rng;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// constant string that is used to identify the JWT secret key in `KeyStore`
pub const JWT_IDENTIFIER: &str = "auth-jwt-private";
/// Admin permissions
pub static ADMIN: &[&str] = &["read", "write", "sign", "admin"];
/// Signing permissions
pub static SIGN: &[&str] = &["read", "write", "sign"];
/// Writing permissions
pub static WRITE: &[&str] = &["read", "write"];
/// Reading permissions
pub static READ: &[&str] = &["read"];

/// Error enumeration for Authentication
#[derive(Debug, Error, Serialize, Deserialize)]
pub enum Error {
    /// Filecoin Method does not exist
    #[error("Filecoin method does not exist")]
    MethodParam,
    /// Invalid permissions to use specified method
    #[error("Incorrect permissions to access method")]
    InvalidPermissions,
    /// Missing authentication header
    #[error("Missing authentication header")]
    NoAuthHeader,
    #[error("{0}")]
    Other(String),
}

/// Claim structure for JWT Tokens
#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    #[serde(rename = "Allow")]
    allow: Vec<String>,
    // Expiration time (as UTC timestamp)
    exp: usize,
}

/// Create a new JWT Token
pub fn create_token(perms: Vec<String>, key: &[u8], token_exp: Duration) -> JWTResult<String> {
    let exp_time = Utc::now() + token_exp;
    let payload = Claims {
        allow: perms,
        exp: exp_time.timestamp() as usize,
    };
    encode(&Header::default(), &payload, &EncodingKey::from_secret(key))
}

/// Verify JWT Token and return the allowed permissions from token
pub fn verify_token(token: &str, key: &[u8]) -> JWTResult<Vec<String>> {
    let validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::default());
    let token = decode::<Claims>(token, &DecodingKey::from_secret(key), &validation)?;
    Ok(token.claims.allow)
}

pub fn generate_priv_key() -> KeyInfo {
    let priv_key = rand::thread_rng().gen::<[u8; 32]>();
    // This is temporary use of bls key as placeholder, need to update keyinfo to use string
    // instead of keyinfo for key type
    KeyInfo::new(SignatureType::Bls, priv_key.to_vec())
}
