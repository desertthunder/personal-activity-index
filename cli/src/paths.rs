use pai_core::{PaiError, Result};
use std::path::PathBuf;

/// Resolves the database file path, with XDG fallback
///
/// Priority order:
/// 1. Explicit path provided via `-d` flag
/// 2. $XDG_DATA_HOME/pai/pai.db
/// 3. $HOME/.local/share/pai/pai.db
pub fn resolve_db_path(explicit_path: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = explicit_path {
        return Ok(path);
    }

    if let Some(data_home) = dirs::data_dir() {
        return Ok(data_home.join("pai").join("pai.db"));
    }

    Err(PaiError::Config(
        "Unable to determine database path: no XDG_DATA_HOME or HOME set".to_string(),
    ))
}

/// Resolves the config directory path, with XDG fallback
///
/// Priority order:
/// 1. Explicit directory provided via `-C` flag
/// 2. $XDG_CONFIG_HOME/pai
/// 3. $HOME/.config/pai
pub fn resolve_config_dir(explicit_dir: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(dir) = explicit_dir {
        return Ok(dir);
    }

    if let Some(config_home) = dirs::config_dir() {
        return Ok(config_home.join("pai"));
    }

    Err(PaiError::Config(
        "Unable to determine config directory: no XDG_CONFIG_HOME or HOME set".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn resolve_db_path_with_explicit() {
        let explicit = Some(PathBuf::from("/custom/path/db.sqlite"));
        let result = resolve_db_path(explicit).unwrap();
        assert_eq!(result, Path::new("/custom/path/db.sqlite"));
    }

    #[test]
    fn resolve_db_path_falls_back() {
        let result = resolve_db_path(None);
        assert!(result.is_ok());

        let path = result.unwrap();
        assert!(path.ends_with("pai/pai.db"));
    }

    #[test]
    fn resolve_config_dir_with_explicit() {
        let explicit = Some(PathBuf::from("/custom/config"));
        let result = resolve_config_dir(explicit).unwrap();
        assert_eq!(result, Path::new("/custom/config"));
    }

    #[test]
    fn resolve_config_dir_falls_back() {
        let result = resolve_config_dir(None);
        assert!(result.is_ok());

        let path = result.unwrap();
        assert!(path.ends_with("pai"));
    }
}
