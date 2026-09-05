use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{env, fs, path::PathBuf};

fn default_base_url() -> String {
    "https://inference.dahl.global/v1".into()
}

fn default_model() -> String {
    "MiniMaxAI/MiniMax-M2.7".into()
}

fn default_temperature() -> f32 {
    0.7
}

fn is_default_base_url(value: &str) -> bool {
    value == "https://inference.dahl.global/v1"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(
        default = "default_base_url",
        skip_serializing_if = "is_default_base_url"
    )]
    pub base_url: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: default_base_url(),
            model: default_model(),
            temperature: default_temperature(),
        }
    }
}

impl Config {
    /// Load configuration with precedence:
    /// 1. built-in defaults
    /// 2. `dahl.json` (next to the executable, then the working directory)
    /// 3. environment variables, which override the file
    pub fn load() -> Result<Self> {
        let mut config = Self::load_file_or_default()?;
        config.normalize();
        config.apply_env_overrides();
        config.normalize();
        config.validate()?;
        Ok(config)
    }

    fn load_file_or_default() -> Result<Self> {
        for path in Self::candidate_paths() {
            if path.exists() {
                let bytes = fs::read(&path)
                    .with_context(|| format!("reading config file {}", path.display()))?;
                return Self::parse_file(&bytes)
                    .with_context(|| format!("parsing config file {}", path.display()));
            }
        }
        Ok(Self::default())
    }

    fn candidate_paths() -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Ok(exe) = env::current_exe() {
            if let Some(dir) = exe.parent() {
                paths.push(dir.join("dahl.json"));
            }
        }
        paths.push(PathBuf::from("dahl.json"));
        paths
    }

    fn parse_file(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).context(
            "invalid dahl.json: expected JSON with optional api_key, base_url, model, temperature",
        )
    }

    fn apply_env_overrides(&mut self) {
        Self::apply_overrides(self, |key| env::var(key).ok());
    }

    fn apply_overrides(config: &mut Self, get: impl Fn(&str) -> Option<String>) {
        if let Some(value) = nonempty_override(get("DAHL_API_KEY")) {
            config.api_key = Some(value);
        }
        if let Some(value) = nonempty_override(get("DAHL_BASE_URL")) {
            config.base_url = value;
        }
        if let Some(value) = nonempty_override(get("DAHL_MODEL")) {
            config.model = value;
        }
    }

    fn normalize(&mut self) {
        if let Some(key) = &mut self.api_key {
            *key = key.trim().to_string();
            if key.is_empty() {
                self.api_key = None;
            }
        }
        self.base_url = self.base_url.trim().trim_end_matches('/').to_string();
        self.model = self.model.trim().to_string();
    }

    fn validate(&self) -> Result<()> {
        if self.model.is_empty() {
            anyhow::bail!("model is empty — set it in dahl.json or DAHL_MODEL");
        }
        if self.base_url.is_empty() {
            anyhow::bail!("base_url is empty — set it in dahl.json or DAHL_BASE_URL");
        }
        Ok(())
    }
}

fn nonempty_override(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dahl_is_the_default_openai_compatible_api() {
        assert_eq!(
            super::default_base_url(),
            "https://inference.dahl.global/v1"
        );
    }

    #[test]
    fn missing_fields_use_defaults() {
        let config: Config = serde_json::from_str(r#"{"api_key":"k"}"#).unwrap();
        assert_eq!(config.model, default_model());
        assert_eq!(config.temperature, 0.7);
        assert_eq!(config.base_url, default_base_url());
    }

    #[test]
    fn env_overrides_file_values() {
        let mut config: Config = serde_json::from_str(
            r#"{"api_key":"file-key","base_url":"https://file.example/v1","model":"file-model"}"#,
        )
        .unwrap();
        Config::apply_overrides(&mut config, |key| match key {
            "DAHL_API_KEY" => Some("env-key".into()),
            "DAHL_BASE_URL" => Some("https://env.example/v1".into()),
            "DAHL_MODEL" => Some("env-model".into()),
            _ => None,
        });
        assert_eq!(config.api_key.as_deref(), Some("env-key"));
        assert_eq!(config.base_url, "https://env.example/v1");
        assert_eq!(config.model, "env-model");
    }

    #[test]
    fn empty_env_does_not_override_file() {
        let mut config: Config = serde_json::from_str(
            r#"{"api_key":"file-key","base_url":"https://file.example/v1","model":"file-model"}"#,
        )
        .unwrap();
        Config::apply_overrides(&mut config, |_| Some("   ".into()));
        assert_eq!(config.api_key.as_deref(), Some("file-key"));
        assert_eq!(config.base_url, "https://file.example/v1");
        assert_eq!(config.model, "file-model");
    }

    #[test]
    fn invalid_json_is_an_error() {
        assert!(Config::parse_file(b"not json").is_err());
        assert!(Config::parse_file(br#"{"temperature":"hot"}"#).is_err());
    }

    #[test]
    fn empty_model_is_rejected() {
        let mut config = Config::default();
        config.model.clear();
        assert!(config.validate().is_err());
    }
}
