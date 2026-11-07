use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_threshold_ms")]
    pub threshold_ms: u64,

    #[serde(default = "default_margin_top")]
    pub margin_top: i32,

    #[serde(default = "default_margin_right")]
    pub margin_right: i32,

    #[serde(default = "default_font_size")]
    pub font_size: f32,

    #[serde(default = "default_max_columns")]
    pub max_columns: usize,

    #[serde(default = "default_niri_config_path")]
    pub niri_config_path: PathBuf,
}

fn default_threshold_ms() -> u64 {
    200
}

fn default_margin_top() -> i32 {
    24
}

fn default_margin_right() -> i32 {
    24
}

fn default_font_size() -> f32 {
    13.0
}

fn default_max_columns() -> usize {
    2
}

fn default_niri_config_path() -> PathBuf {
    std::env::var("NIRI_CONFIG")
        .map(expand_tilde)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".config/niri/config.kdl")
        })
}

pub fn expand_tilde<P: AsRef<Path>>(path: P) -> PathBuf {
    let p = path.as_ref();
    if let Ok(stripped) = p.strip_prefix("~") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(stripped);
        }
    }
    p.to_path_buf()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            threshold_ms: default_threshold_ms(),
            margin_top: default_margin_top(),
            margin_right: default_margin_right(),
            font_size: default_font_size(),
            max_columns: default_max_columns(),
            niri_config_path: default_niri_config_path(),
        }
    }
}

impl AppConfig {
    pub fn load(custom_path: Option<&Path>) -> (Self, Option<PathBuf>) {
        let mut config = Self::default();
        let loaded_path = if let Some(p) = custom_path {
            let expanded = expand_tilde(p);
            if expanded.exists() {
                Some(expanded)
            } else {
                eprintln!(
                    "[niri-whichkey] Specified config file not found: {}",
                    p.display()
                );
                None
            }
        } else {
            find_default_config()
        };

        if let Some(ref path) = loaded_path {
            match fs::read_to_string(path) {
                Ok(content) => match serde_yaml::from_str::<AppConfig>(&content) {
                    Ok(mut parsed) => {
                        println!(
                            "[niri-whichkey] Loaded configuration from {}",
                            path.display()
                        );
                        parsed.niri_config_path = expand_tilde(&parsed.niri_config_path);
                        config = parsed;
                    }
                    Err(e) => {
                        eprintln!(
                            "[niri-whichkey] Error parsing config file {}: {}",
                            path.display(),
                            e
                        );
                    }
                },
                Err(e) => {
                    eprintln!(
                        "[niri-whichkey] Could not read config file {}: {}",
                        path.display(),
                        e
                    );
                }
            }
        }

        (config, loaded_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_yaml_deserialize() {
        let yaml = r#"
threshold_ms: 350
margin_top: 40
margin_right: 30
font_size: 14.5
max_columns: 1
niri_config_path: "/tmp/test.kdl"
"#;
        let config: AppConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.threshold_ms, 350);
        assert_eq!(config.margin_top, 40);
        assert_eq!(config.margin_right, 30);
        assert_eq!(config.font_size, 14.5);
        assert_eq!(config.max_columns, 1);
        assert_eq!(config.niri_config_path, PathBuf::from("/tmp/test.kdl"));
    }

    #[test]
    fn test_expand_tilde() {
        if let Ok(home) = std::env::var("HOME") {
            let expanded = expand_tilde("~/test/file.kdl");
            assert_eq!(expanded, PathBuf::from(home).join("test/file.kdl"));
        }
    }
}

fn find_default_config() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        let p = PathBuf::from(&xdg).join("niri-whichkey/config.yaml");
        if p.exists() {
            return Some(p);
        }
        let p = PathBuf::from(&xdg).join("niri-whichkey/config.yml");
        if p.exists() {
            return Some(p);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let p = PathBuf::from(&home).join(".config/niri-whichkey/config.yaml");
        if p.exists() {
            return Some(p);
        }
        let p = PathBuf::from(&home).join(".config/niri-whichkey/config.yml");
        if p.exists() {
            return Some(p);
        }
    }
    None
}
