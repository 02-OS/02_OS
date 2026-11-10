//! Local skill documents and agent identity types.
//!
//! A skill names a version and a list of tools that already exist in this
//! workspace. Loading a skill does not open a socket and does not fetch anything.

mod identity;
mod load;
mod tools;

pub use identity::{constant_time_eq, AgentId, CapabilityToken, SharedContextScope};
pub use load::{load_skill, Skill, SkillError};
pub use tools::known_tools;
