use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    /// macOS: use --apple-use-keychain with ssh-add
    #[serde(default = "default_apple_keychain")]
    pub apple_keychain: bool,

    #[serde(rename = "rule", default)]
    pub rules: Vec<Rule>,
}

fn default_apple_keychain() -> bool {
    cfg!(target_os = "macos")
}

#[derive(Debug, Deserialize, Clone)]
pub struct Rule {
    pub host: String,
    #[serde(rename = "match")]
    pub match_pattern: Option<String>,
    pub key: String,
    pub email: Option<String>,
    pub name: Option<String>,
    /// Optional SSH port override (e.g. 222 for non-standard SSH)
    pub port: Option<u16>,
    /// If true, `pickey init` may overwrite this rule when regenerating
    #[serde(default)]
    pub auto: bool,
}

impl Rule {
    /// Returns the key path with ~ expanded to the home directory.
    pub fn expanded_key(&self) -> PathBuf {
        expand_tilde(&self.key)
    }
}

/// Expand ~ at the start of a path to the user's home directory.
pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    if path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    PathBuf::from(path)
}

/// Default config file path: ~/.config/pickey/config.toml
/// We use ~/.config explicitly (XDG-style) rather than the OS config dir,
/// because ~/Library/Application Support/ is unexpected for CLI tools on macOS.
pub fn default_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join(".config")
        .join("pickey")
        .join("config.toml")
}

/// Load config from the given path (or default).
pub fn load_config(path: Option<&Path>) -> Result<Config, String> {
    let config_path = match path {
        Some(p) => p.to_path_buf(),
        None => default_config_path(),
    };

    if !config_path.exists() {
        return Err(format!("Config file not found: {}", config_path.display()));
    }

    let contents = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read {}: {}", config_path.display(), e))?;

    let config: Config = toml::from_str(&contents)
        .map_err(|e| format!("Failed to parse {}: {}", config_path.display(), e))?;

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_config() {
        let toml = r#"
[[rule]]
host = "github.com"
match = "VolvoGroup-Internal/*"
key = "~/.ssh/id_volvo"
email = "simeon@volvo.com"
name = "Simeon Volvo"

[[rule]]
host = "github.com"
match = "MyPersonalOrg/*"
key = "~/.ssh/id_personal"

[[rule]]
host = "ssh.dev.azure.com"
match = "v3/ClientX/**"
key = "~/.ssh/id_clientx"
email = "simeon@clientx.com"

[[rule]]
host = "gitlab.selfhosted.client.com"
key = "~/.ssh/id_client_gitlab"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.rules.len(), 4);
        assert_eq!(config.rules[0].host, "github.com");
        assert_eq!(
            config.rules[0].match_pattern.as_deref(),
            Some("VolvoGroup-Internal/*")
        );
        assert_eq!(config.rules[0].email.as_deref(), Some("simeon@volvo.com"));
        assert!(config.rules[1].email.is_none());
        assert!(config.rules[3].match_pattern.is_none());
    }

    #[test]
    fn tilde_expansion() {
        let expanded = expand_tilde("~/.ssh/id_rsa");
        assert!(expanded.to_str().unwrap().contains(".ssh/id_rsa"));
        assert!(!expanded.to_str().unwrap().starts_with('~'));
    }

    #[test]
    fn no_tilde() {
        let expanded = expand_tilde("/absolute/path/key");
        assert_eq!(expanded, PathBuf::from("/absolute/path/key"));
    }

    #[test]
    fn apple_keychain_default() {
        let toml = r#"
[[rule]]
host = "github.com"
key = "~/.ssh/id"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        // On macOS this should be true, on Linux false
        if cfg!(target_os = "macos") {
            assert!(config.apple_keychain);
        } else {
            assert!(!config.apple_keychain);
        }
    }
}
