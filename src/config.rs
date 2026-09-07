use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{env, fs, path::PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    pub base_url: &'static str,
    pub default_model: &'static str,
    pub env_key: &'static str,
}

pub const PROVIDERS: &[Provider] = &[
    Provider {
        id: "dahl",
        name: "Dahl",
        base_url: "https://inference.dahl.global/v1",
        default_model: "MiniMaxAI/MiniMax-M2.7",
        env_key: "DAHL_API_KEY",
    },
    Provider {
        id: "apinex",
        name: "APInex",
        base_url: "https://api.apinex.bond/v1",
        default_model: "gpt-5-6-terra",
        env_key: "APINEX_API_KEY",
    },
];

pub fn find_provider(id_or_name: &str) -> Option<&'static Provider> {
    let lower = id_or_name.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return None;
    }
    if let Some(p) = PROVIDERS
        .iter()
        .find(|p| p.id.eq_ignore_ascii_case(&lower) || p.name.eq_ignore_ascii_case(&lower))
    {
        return Some(p);
    }
    let starts: Vec<&Provider> = PROVIDERS
        .iter()
        .filter(|p| {
            p.id.to_ascii_lowercase().starts_with(&lower)
                || p.name.to_ascii_lowercase().starts_with(&lower)
        })
        .collect();
    if starts.len() == 1 {
        return Some(starts[0]);
    }
    None
}

fn default_temperature() -> f32 {
    0.7
}

fn is_default_temperature(val: &f32) -> bool {
    (*val - 0.7).abs() < f32::EPSILON
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dahl_api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apinex_api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub base_url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
    #[serde(default = "default_temperature", skip_serializing_if = "is_default_temperature")]
    pub temperature: f32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub provider: String,
}

impl Default for Config {
    fn default() -> Self {
        let dahl_key = env::var("DAHL_API_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty());
        let apinex_key = env::var("APINEX_API_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty());

        let has_dahl = dahl_key.is_some();
        let has_apinex = apinex_key.is_some();
        let provider_id = if !has_dahl && has_apinex {
            "apinex"
        } else {
            "dahl"
        };
        let provider = find_provider(provider_id).unwrap_or(&PROVIDERS[0]);

        let base_url = if provider.id == "apinex" {
            env::var("APINEX_BASE_URL").unwrap_or_else(|_| provider.base_url.to_string())
        } else {
            env::var("DAHL_BASE_URL").unwrap_or_else(|_| provider.base_url.to_string())
        };

        let model = if provider.id == "apinex" {
            env::var("APINEX_MODEL").unwrap_or_else(|_| provider.default_model.to_string())
        } else {
            env::var("DAHL_MODEL").unwrap_or_else(|_| provider.default_model.to_string())
        };

        let active_key = if provider.id == "apinex" {
            apinex_key.clone()
        } else {
            dahl_key.clone()
        };

        Self {
            dahl_api_key: dahl_key,
            apinex_api_key: apinex_key,
            api_key: active_key,
            base_url,
            model,
            temperature: 0.7,
            provider: provider.id.to_string(),
        }
    }
}

impl Config {
    pub fn api_key_for_provider(&self, provider_id: &str) -> Option<String> {
        match provider_id {
            "dahl" => self
                .dahl_api_key
                .clone()
                .or_else(|| env::var("DAHL_API_KEY").ok())
                .filter(|s| !s.trim().is_empty()),
            "apinex" => self
                .apinex_api_key
                .clone()
                .or_else(|| env::var("APINEX_API_KEY").ok())
                .filter(|s| !s.trim().is_empty()),
            _ => None,
        }
    }

    pub fn set_provider(&mut self, provider_id: &str) -> Result<(), String> {
        let Some(provider) = find_provider(provider_id) else {
            return Err(format!("unknown provider: '{provider_id}'"));
        };
        self.provider = provider.id.to_string();
        self.base_url = if provider.id == "apinex" {
            env::var("APINEX_BASE_URL").unwrap_or_else(|_| provider.base_url.to_string())
        } else {
            env::var("DAHL_BASE_URL").unwrap_or_else(|_| provider.base_url.to_string())
        };
        self.model = if provider.id == "apinex" {
            env::var("APINEX_MODEL").unwrap_or_else(|_| provider.default_model.to_string())
        } else {
            env::var("DAHL_MODEL").unwrap_or_else(|_| provider.default_model.to_string())
        };
        self.api_key = self.api_key_for_provider(provider.id);
        Ok(())
    }

    pub fn load() -> Result<Self> {
        let executable_dir = env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(PathBuf::from));
        let candidates = [
            executable_dir.as_ref().map(|dir| dir.join("config.json")),
            Some(PathBuf::from("config.json")),
            executable_dir.as_ref().map(|dir| dir.join("dahl.json")),
            Some(PathBuf::from("dahl.json")),
        ];
        for candidate in candidates {
            if let Some(path) = candidate {
                if path.exists() {
                    let bytes =
                        fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
                    let mut config: Self = serde_json::from_slice(&bytes)
                        .with_context(|| format!("parsing {}", path.display()))?;

                    if let Ok(key) = env::var("DAHL_API_KEY") {
                        if !key.trim().is_empty() {
                            config.dahl_api_key = Some(key);
                        }
                    }
                    if let Ok(key) = env::var("APINEX_API_KEY") {
                        if !key.trim().is_empty() {
                            config.apinex_api_key = Some(key);
                        }
                    }
                    if let Some(key) = &config.api_key {
                        if key.starts_with("sk-apx") {
                            if config.apinex_api_key.is_none() {
                                config.apinex_api_key = Some(key.clone());
                            }
                        } else if config.dahl_api_key.is_none() {
                            config.dahl_api_key = Some(key.clone());
                        }
                    }

                    let has_dahl = config
                        .dahl_api_key
                        .as_deref()
                        .map(str::trim)
                        .is_some_and(|s| !s.is_empty());
                    let has_apinex = config
                        .apinex_api_key
                        .as_deref()
                        .map(str::trim)
                        .is_some_and(|s| !s.is_empty());

                    let provider_id = if config.provider.is_empty() {
                        if !has_dahl && has_apinex {
                            "apinex"
                        } else {
                            "dahl"
                        }
                    } else {
                        &config.provider
                    };
                    let provider = find_provider(provider_id).unwrap_or(&PROVIDERS[0]);
                    config.provider = provider.id.to_string();

                    if config.base_url.is_empty() {
                        config.base_url = if provider.id == "apinex" {
                            env::var("APINEX_BASE_URL")
                                .unwrap_or_else(|_| provider.base_url.to_string())
                        } else {
                            env::var("DAHL_BASE_URL")
                                .unwrap_or_else(|_| provider.base_url.to_string())
                        };
                    }

                    if config.model.is_empty() {
                        config.model = if provider.id == "apinex" {
                            env::var("APINEX_MODEL")
                                .unwrap_or_else(|_| provider.default_model.to_string())
                        } else {
                            env::var("DAHL_MODEL")
                                .unwrap_or_else(|_| provider.default_model.to_string())
                        };
                    }

                    config.api_key = config.api_key_for_provider(&config.provider);
                    return Ok(config);
                }
            }
        }
        Ok(Self::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_registry_contains_dahl_and_apinex() {
        assert_eq!(PROVIDERS.len(), 2);
        assert_eq!(PROVIDERS[0].id, "dahl");
        assert_eq!(PROVIDERS[0].base_url, "https://inference.dahl.global/v1");
        assert_eq!(PROVIDERS[1].id, "apinex");
        assert_eq!(PROVIDERS[1].base_url, "https://api.apinex.bond/v1");
    }

    #[test]
    fn find_provider_matches_exact_and_prefix() {
        assert_eq!(find_provider("dahl").unwrap().id, "dahl");
        assert_eq!(find_provider("DAHL").unwrap().id, "dahl");
        assert_eq!(find_provider("apinex").unwrap().id, "apinex");
        assert_eq!(find_provider("APInex").unwrap().id, "apinex");
        assert_eq!(find_provider("api").unwrap().id, "apinex");
        assert_eq!(find_provider("d").unwrap().id, "dahl");
        assert!(find_provider("unknown").is_none());
    }

    #[test]
    fn switching_provider_updates_base_url_and_model() {
        let mut config = Config::default();
        config.set_provider("apinex").unwrap();
        assert_eq!(config.provider, "apinex");
        assert_eq!(config.base_url, "https://api.apinex.bond/v1");
        assert_eq!(config.model, "gpt-5-6-terra");

        config.set_provider("dahl").unwrap();
        assert_eq!(config.provider, "dahl");
        assert_eq!(config.base_url, "https://inference.dahl.global/v1");
        assert_eq!(config.model, "MiniMaxAI/MiniMax-M2.7");
    }
}
