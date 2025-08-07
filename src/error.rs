use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization/deserialization error: {0}")]
    SerdeJson(#[from] serde_json::Error),

    #[error("TOML deserialization error: {0}")]
    TomlDe(#[from] toml::de::Error),

    #[error("TOML serialization error: {0}")]
    TomlSer(#[from] toml::ser::Error),

    #[error("File filtering error: {0}")]
    Ignore(#[from] ignore::Error),

    #[error("Git error: {0}")]
    Git(#[from] git2::Error),

    #[error("Path stripping error: {0}")]
    StripPrefix(#[from] std::path::StripPrefixError),

    #[error("Glob pattern error: {0}")]
    Glob(#[from] globset::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
