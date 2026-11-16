//! Agent identity stays on this machine.
//!
//! A capability token is stored only as a SHA-256 digest. The secret that
//! produced the digest is not kept on the value.

use chrono::Utc;
use sha2::{Digest, Sha256};
use std::fmt;

use crate::load::{Skill, SkillError};

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

    /// Tag written onto memories/facts created inside this context scope.
    pub fn isolation_tag(&self, agent: &AgentId) -> String {
        match self {
            Self::Private => format!("scope:private:{}", agent.as_str()),
            Self::Repository(id) => format!("scope:repo:{id}"),
            Self::Session(id) => format!("scope:session:{id}"),
        }
    }

    /// Whether an agent running under `self` may read an item carrying `tags`.
    pub fn can_read_tags(&self, agent: &AgentId, tags: &[String]) -> bool {
        let own_private = format!("scope:private:{}", agent.as_str());
        for tag in tags {
            if let Some(owner) = tag.strip_prefix("scope:private:") {
                if !matches!(self, Self::Private) || owner != agent.as_str() {
                    return false;
                }
            } else if let Some(sess) = tag.strip_prefix("scope:session:") {
                if !matches!(self, Self::Session(active) if active == sess) {
                    return false;
                }
            } else if let Some(repo) = tag.strip_prefix("scope:repo:") {
                match self {
                    Self::Repository(active) if active == repo => {}
                    _ => return false,
                }
            }
        }
        match self {
            Self::Private => tags.iter().any(|t| t == &own_private),
            Self::Session(id) => {
                let expected = format!("scope:session:{id}");
                tags.iter().any(|t| t == &expected)
            }
            Self::Repository(_) => true,
        }
    }

    /// Whether accessing repository `repo_id` is allowed by this scope.
    pub fn allows_repository(&self, repo_id: &str) -> bool {
        match self {
            Self::Repository(allowed) => allowed == repo_id,
            Self::Private | Self::Session(_) => true,
        }
    }
}

/// SHA-256 of a capability secret with optional Unix expiration timestamp. The secret is never stored.
#[derive(Clone, Eq)]
pub struct CapabilityToken {
    hash: [u8; 32],
    expires_at_unix: Option<i64>,
}

impl CapabilityToken {
    pub fn seal(secret: &str) -> Result<Self, SkillError> {
        Self::seal_internal(secret, None)
    }

    pub fn seal_with_ttl(secret: &str, ttl_secs: i64) -> Result<Self, SkillError> {
        let expires_at = Utc::now().timestamp().saturating_add(ttl_secs);
        Self::seal_internal(secret, Some(expires_at))
    }

    pub fn seal_with_expiry(secret: &str, expires_at_unix: i64) -> Result<Self, SkillError> {
        Self::seal_internal(secret, Some(expires_at_unix))
    }

    fn seal_internal(secret: &str, expires_at_unix: Option<i64>) -> Result<Self, SkillError> {
        if secret.is_empty() || secret.len() > 256 {
            return Err(SkillError::EmptyToken);
        }
        let digest = Sha256::digest(secret.as_bytes());
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&digest);
        Ok(Self {
            hash,
            expires_at_unix,
        })
    }

    pub fn expires_at(&self) -> Option<i64> {
        self.expires_at_unix
    }

    pub fn is_expired_at(&self, now_unix: i64) -> bool {
        matches!(self.expires_at_unix, Some(exp) if now_unix >= exp)
    }

    pub fn is_expired(&self) -> bool {
        self.is_expired_at(Utc::now().timestamp())
    }

    pub fn matches(&self, secret: &str) -> bool {
        match Self::seal_internal(secret, self.expires_at_unix) {
            Ok(other) => constant_time_eq(&self.hash, &other.hash),
            Err(_) => false,
        }
    }

    pub fn matches_valid(&self, secret: &str) -> bool {
        !self.is_expired() && self.matches(secret)
    }

    pub fn matches_valid_at(&self, secret: &str, now_unix: i64) -> bool {
        !self.is_expired_at(now_unix) && self.matches(secret)
    }

    pub fn hex(&self) -> String {
        hex::encode(self.hash)
    }
}

impl PartialEq for CapabilityToken {
    fn eq(&self, other: &Self) -> bool {
        constant_time_eq(&self.hash, &other.hash) && self.expires_at_unix == other.expires_at_unix
    }
}

impl fmt::Debug for CapabilityToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "CapabilityToken({})", self.hex())
    }
}

/// Active skill, identity, capability token, and context isolation policy bound to a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPolicy {
    pub agent_id: AgentId,
    pub skill: Skill,
    pub scope: SharedContextScope,
    pub token: Option<CapabilityToken>,
    pub presented_secret: Option<String>,
}

impl SessionPolicy {
    pub fn new(agent_id: AgentId, skill: Skill, scope: SharedContextScope) -> Self {
        Self {
            agent_id,
            skill,
            scope,
            token: None,
            presented_secret: None,
        }
    }

    pub fn with_capability(mut self, token: CapabilityToken, presented_secret: &str) -> Self {
        self.token = Some(token);
        self.presented_secret = Some(presented_secret.to_string());
        self
    }

    pub fn authorize_token(&self) -> Result<(), SkillError> {
        let Some(token) = &self.token else {
            return Ok(());
        };
        if token.is_expired() {
            return Err(SkillError::ExpiredToken);
        }
        let secret = self.presented_secret.as_deref().unwrap_or("");
        if !token.matches(secret) {
            return Err(SkillError::InvalidToken);
        }
        Ok(())
    }

    pub fn allows_tool(&self, tool: &str) -> Result<(), SkillError> {
        self.authorize_token()?;
        if self.skill.tools.iter().any(|allowed| allowed == tool) {
            Ok(())
        } else {
            Err(SkillError::UnauthorizedTool(tool.to_string()))
        }
    }

    pub fn scope_tag(&self) -> String {
        self.scope.isolation_tag(&self.agent_id)
    }

    pub fn can_read_tags(&self, tags: &[String]) -> bool {
        self.scope.can_read_tags(&self.agent_id, tags)
    }

    pub fn allows_repository(&self, repo_id: &str) -> bool {
        self.scope.allows_repository(repo_id)
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

    #[test]
    fn capability_ttl_and_session_policy_enforce_allowlist_and_scope() {
        let unexpired = CapabilityToken::seal_with_ttl("secret", 300).unwrap();
        assert!(unexpired.matches_valid("secret"));
        let expired = CapabilityToken::seal_with_expiry("secret", 10).unwrap();
        assert!(!expired.matches_valid_at("secret", 10));
        assert!(!expired.matches_valid("secret"));

        let agent_a = AgentId::parse("reviewer").unwrap();
        let agent_b = AgentId::parse("indexer").unwrap();
        let skill = Skill {
            name: "review".into(),
            version: "0.1.0".into(),
            tools: vec!["repository_search".into(), "memory_get".into()],
        };
        let policy = SessionPolicy::new(agent_a.clone(), skill, SharedContextScope::Private)
            .with_capability(unexpired, "secret");
        assert!(policy.allows_tool("repository_search").is_ok());
        assert!(matches!(
            policy.allows_tool("memory_write"),
            Err(SkillError::UnauthorizedTool(_))
        ));

        let own_tags = vec![policy.scope_tag()];
        assert!(policy.can_read_tags(&own_tags));
        let other_tags = vec![SharedContextScope::Private.isolation_tag(&agent_b)];
        assert!(!policy.can_read_tags(&other_tags));
        assert!(!policy.can_read_tags(&[]));

        let repo_scope = SharedContextScope::repository("repo_a").unwrap();
        assert!(repo_scope.allows_repository("repo_a"));
        assert!(!repo_scope.allows_repository("repo_b"));
    }
}
