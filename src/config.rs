use crate::error::{Error, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize, Debug, Default)]
pub struct Config {
    // Placeholder for future configuration options
}

pub fn load_config(path: &Path) -> Result<Config> {
    if !path.exists() {
        return Ok(Config::default());
    }

    let content = std::fs::read_to_string(path).map_err(Error::Io)?;
    toml::from_str(&content).map_err(|e| Error::Config(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_load_config_non_existent() {
        let path = Path::new("non_existent_config.toml");
        let config = load_config(path).unwrap();
        // Should return default config
        assert!(matches!(config, Config { .. }));
    }

    #[test]
    fn test_load_config_empty_file() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "").unwrap();
        let config = load_config(file.path()).unwrap();
        assert!(matches!(config, Config { .. }));
    }

    #[test]
    fn test_load_config_invalid_toml() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "this is not toml").unwrap();
        let result = load_config(file.path());
        assert!(result.is_err());
        if let Err(Error::Config(_)) = result {
            // correct error type
        } else {
            panic!("Expected Config error");
        }
    }
}
