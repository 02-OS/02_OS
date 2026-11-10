//! Agent identity stays on this machine.
//!
//! A capability token is stored only as a SHA-256 digest. The secret that
//! produced the digest is not kept on the value.

use sha2::{Digest, Sha256};
use std::fmt;

use crate::load::SkillError;

/// Compare two equal-length byte strings without stopping at the first difference.
pub fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (left_byte, right_byte) in left.iter().zip(right.iter()) {
        diff |= left_byte ^ right_byte;
    }
    diff == 0
}

/// Local agent name. It is not a filesystem path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentId(String);

impl AgentId {
    pub fn parse(raw: &str) -> Result<Self, SkillError> {
        if !valid_label(raw) {
            return Err(SkillError::InvalidAgentId);
        }
        Ok(Self(raw.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// How far this agent's context may be shared. Every scope is local.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SharedContextScope {
    /// Context stays with this agent.
    Private,
    /// Context may be read by agents bound to one repository id.
    Repository(String),
    /// Context may be read by agents bound to one session id.
    Session(String),
}

impl SharedContextScope {
    pub fn repository(id: &str) -> Result<Self, SkillError> {
        if !valid_label(id) {
            return Err(SkillError::InvalidScope);
        }
        Ok(Self::Repository(id.to_string()))
    }

    pub fn session(id: &str) -> Result<Self, SkillError> {
        if !valid_label(id) {
            return Err(SkillError::InvalidScope);
        }
        Ok(Self::Session(id.to_string()))
    }
}

/// SHA-256 of a capability secret. The secret itself is not stored.
#[derive(Clone, Eq)]
pub struct CapabilityToken {
    hash: [u8; 32],
}

impl CapabilityToken {
    pub fn seal(secret: &str) -> Result<Self, SkillError> {
        if secret.is_empty() || secret.len() > 256 {
            return Err(SkillError::EmptyToken);
        }
        let digest = Sha256::digest(secret.as_bytes());
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&digest);
        Ok(Self { hash })
    }

    pub fn matches(&self, secret: &str) -> bool {
        match Self::seal(secret) {
            Ok(other) => self == &other,
            Err(_) => false,
        }
    }

    pub fn hex(&self) -> String {
        hex::encode(self.hash)
    }
}

impl PartialEq for CapabilityToken {
    fn eq(&self, other: &Self) -> bool {
        constant_time_eq(&self.hash, &other.hash)
    }
}

impl fmt::Debug for CapabilityToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "CapabilityToken({})", self.hex())
    }
}

fn valid_label(raw: &str) -> bool {
    let mut chars = raw.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    raw.len() <= 64
        && first.is_ascii_alphabetic()
        && raw.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_stored_as_a_hash_and_compared_without_the_secret() {
        let secret = "super-secret-token";
        let token = CapabilityToken::seal(secret).expect("seal");
        assert!(token.matches(secret));
        assert!(!token.matches("super-secret-tokem"));
        assert!(!token.matches(""));
        assert!(CapabilityToken::seal("").is_err());
        let rendered = format!("{token:?}");
        assert!(!rendered.contains(secret));
        assert_eq!(token.hex().len(), 64);
        assert_eq!(token, CapabilityToken::seal(secret).expect("again"));
        assert_ne!(token, CapabilityToken::seal("other-secret").expect("other"));
    }

    #[test]
    fn agent_id_and_scope_reject_paths() {
        assert_eq!(AgentId::parse("reviewer").expect("id").as_str(), "reviewer");
        assert!(AgentId::parse("../reviewer").is_err());
        assert!(AgentId::parse("a/b").is_err());
        assert!(AgentId::parse("").is_err());
        assert!(matches!(
            SharedContextScope::repository("repo_1").expect("scope"),
            SharedContextScope::Repository(_)
        ));
        assert!(SharedContextScope::session("../session").is_err());
        assert!(matches!(
            SharedContextScope::Private,
            SharedContextScope::Private
        ));
    }

    #[test]
    fn constant_time_compare_accepts_equal_hashes_only() {
        let left = CapabilityToken::seal("alpha-token").expect("left").hex();
        let right = CapabilityToken::seal("alpha-token").expect("right").hex();
        assert!(constant_time_eq(left.as_bytes(), right.as_bytes()));
        let mut flipped = right.into_bytes();
        flipped[0] ^= 0x01;
        assert!(!constant_time_eq(left.as_bytes(), &flipped));
        assert!(!constant_time_eq(b"short", b"longer"));
    }
}
