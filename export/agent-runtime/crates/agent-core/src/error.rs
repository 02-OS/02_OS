use thiserror::Error;

pub type Result<T> = std::result::Result<T, AgentError>;

#[derive(Error, Debug)]
pub enum AgentError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TOML error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("TOML serialization error: {0}")]
    TomlSer(#[from] toml::ser::Error),

    #[error("Git error: {0}")]
    Git(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Repository not found: {0}")]
    RepositoryNotFound(String),

    #[error("Repository not initialized. Run '02 init' first.")]
    RepositoryNotInitialized,

    #[error("Security violation: {0}")]
    Security(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("General error: {0}")]
    General(String),
}
