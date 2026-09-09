use crate::api::client::ApiClient;
use crate::api::providers::ProviderKind;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{env, fs, path::PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    pub kind: ProviderKind,
    pub base_url: &'static str,
    pub default_model: &'static str,
    pub env_key: &'static str,
}

pub const PROVIDERS: &[Provider] = &[
    Provider {
        id: "dahl",
        name: "Dahl",
        kind: ProviderKind::OpenAICompatible,
        base_url: "https://inference.dahl.global/v1",
        default_model: "MiniMaxAI/MiniMax-M2.7",
        env_key: "DAHL_API_KEY",
    },
    Provider {
        id: "apinex",
        name: "APInex",
        kind: ProviderKind::OpenAICompatible,
        base_url: "https://api.apinex.bond/v1",
        default_model: "gpt-5-6-terra",
        env_key: "APINEX_API_KEY",
    },
    Provider {
        id: "openai",
        name: "OpenAI",
        kind: ProviderKind::OpenAICompatible,
        base_url: "https://api.openai.com/v1",
        default_model: "gpt-4o-mini",
        env_key: "OPENAI_API_KEY",
    },
    Provider {
        id: "anthropic",
        name: "Anthropic",
        kind: ProviderKind::Anthropic,
        base_url: "https://api.anthropic.com/v1",
        default_model: "claude-sonnet-4-5",
        env_key: "ANTHROPIC_API_KEY",
    },
    Provider {
        id: "gemini",
        name: "Google Gemini",
        kind: ProviderKind::Gemini,
        base_url: "https://generativelanguage.googleapis.com/v1beta",
        default_model: "gemini-2.5-flash",
        env_key: "GEMINI_API_KEY",
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
    pub openai_api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anthropic_api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gemini_api_key: Option<String>,
    /// Legacy single-key field: migrated into the active provider's key.
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
        let mut config = Self {
            dahl_api_key: None,
            apinex_api_key: None,
            openai_api_key: None,
            anthropic_api_key: None,
            gemini_api_key: None,
            api_key: None,
            base_url: String::new(),
            model: String::new(),
            temperature: 0.7,
            provider: String::new(),
        };
        // Pick up every provider's key from its environment variable.
        for provider in PROVIDERS {
            if let Ok(key) = env::var(provider.env_key) {
                if !key.trim().is_empty() {
                    config.set_provider_key(provider.id, key);
                }
            }
        }
        // First provider with a key wins (Dahl is first, so it keeps its
        // historical precedence); Dahl is the fallback when nothing is set.
        let provider = PROVIDERS
            .iter()
            .find(|p| config.api_key_for_provider(p.id).is_some())
            .unwrap_or(&PROVIDERS[0]);

        // A provider preset only pins defaults; explicit `{ID}_BASE_URL` /
        // `{ID}_MODEL` environment variables override them per provider.
        config.provider = provider.id.to_string();
        config.base_url = env::var(format!("{}_BASE_URL", provider.id.to_uppercase()))
            .unwrap_or_else(|_| provider.base_url.to_string());
        config.model = env::var(format!("{}_MODEL", provider.id.to_uppercase()))
            .unwrap_or_else(|_| provider.default_model.to_string());
        config.api_key = config.api_key_for_provider(provider.id);
        config
    }
}

impl Config {
    /// Build an [`ApiClient`] for the active provider: its protocol backend,
    /// bound to the configured API key and base URL.
    pub fn api_client(&self) -> ApiClient {
        let kind = find_provider(&self.provider)
            .map(|p| p.kind)
            .unwrap_or(ProviderKind::OpenAICompatible);
        ApiClient::new(kind.build(
            self.api_key.clone().unwrap_or_default(),
            self.base_url.clone(),
        ))
    }

    /// Store a provider's key in its dedicated field.
    fn set_provider_key(&mut self, provider_id: &str, key: String) {
        match provider_id {
            "dahl" => self.dahl_api_key = Some(key),
            "apinex" => self.apinex_api_key = Some(key),
            "openai" => self.openai_api_key = Some(key),
            "anthropic" => self.anthropic_api_key = Some(key),
            "gemini" => self.gemini_api_key = Some(key),
            _ => {}
        }
    }

    pub fn api_key_for_provider(&self, provider_id: &str) -> Option<String> {
        // The dedicated `{id}_api_key` field wins; the provider's `{ID}_API_KEY`
        // environment variable is the fallback.
        let stored = match provider_id {
            "dahl" => self.dahl_api_key.clone(),
            "apinex" => self.apinex_api_key.clone(),
            "openai" => self.openai_api_key.clone(),
            "anthropic" => self.anthropic_api_key.clone(),
            "gemini" => self.gemini_api_key.clone(),
            _ => None,
        };
        let env_key = find_provider(provider_id).map(|p| p.env_key);
        stored
            .or_else(|| env_key.and_then(|key| env::var(key).ok()))
            .filter(|s| !s.trim().is_empty())
    }

    pub fn set_provider(&mut self, provider_id: &str) -> Result<(), String> {
        let Some(provider) = find_provider(provider_id) else {
            return Err(format!("unknown provider: '{provider_id}'"));
        };
        self.provider = provider.id.to_string();
        self.base_url = env::var(format!("{}_BASE_URL", provider.id.to_uppercase()))
            .unwrap_or_else(|_| provider.base_url.to_string());
        self.model = env::var(format!("{}_MODEL", provider.id.to_uppercase()))
            .unwrap_or_else(|_| provider.default_model.to_string());
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

                    // Environment keys always override the file's values.
                    for provider in PROVIDERS {
                        if let Ok(key) = env::var(provider.env_key) {
                            if !key.trim().is_empty() {
                                config.set_provider_key(provider.id, key);
                            }
                        }
                    }
                    // Legacy single `api_key` field: route it into the
                    // original two providers by key prefix.
                    if let Some(key) = &config.api_key {
                        if key.starts_with("sk-apx") {
                            if config.apinex_api_key.is_none() {
                                config.apinex_api_key = Some(key.clone());
                            }
                        } else if config.dahl_api_key.is_none() {
                            config.dahl_api_key = Some(key.clone());
                        }
                    }

                    // No explicit provider? Use the first one that has a key,
                    // falling back to Dahl.
                    let provider_id = if config.provider.is_empty() {
                        PROVIDERS
                            .iter()
                            .find(|p| config.api_key_for_provider(p.id).is_some())
                            .map(|p| p.id)
                            .unwrap_or(PROVIDERS[0].id)
                    } else {
                        &config.provider
                    };
                    let provider = find_provider(provider_id).unwrap_or(&PROVIDERS[0]);
                    config.provider = provider.id.to_string();

                    if config.base_url.is_empty() {
                        config.base_url = env::var(format!("{}_BASE_URL", provider.id.to_uppercase()))
                            .unwrap_or_else(|_| provider.base_url.to_string());
                    }

                    if config.model.is_empty() {
                        config.model = env::var(format!("{}_MODEL", provider.id.to_uppercase()))
                            .unwrap_or_else(|_| provider.default_model.to_string());
                    }

                    // The legacy `api_key` field also acts as the active
                    // provider's key when no dedicated field is present
                    // (e.g. `{"provider": "openai", "api_key": "…"}`).
                    if config.api_key_for_provider(&config.provider).is_none() {
                        if let Some(key) = config.api_key.clone() {
                            if !key.trim().is_empty() {
                                let provider_id = config.provider.clone();
                                config.set_provider_key(&provider_id, key);
                            }
                        }
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
        assert!(PROVIDERS.iter().any(|p| p.id == "dahl"));
        assert!(PROVIDERS.iter().any(|p| p.id == "apinex"));
        let dahl = find_provider("dahl").unwrap();
        assert_eq!(dahl.kind, ProviderKind::OpenAICompatible);
        assert_eq!(dahl.base_url, "https://inference.dahl.global/v1");
        let apinex = find_provider("apinex").unwrap();
        assert_eq!(apinex.kind, ProviderKind::OpenAICompatible);
        assert_eq!(apinex.base_url, "https://api.apinex.bond/v1");
        assert_eq!(find_provider("anthropic").unwrap().kind, ProviderKind::Anthropic);
        assert_eq!(find_provider("gemini").unwrap().kind, ProviderKind::Gemini);
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

    #[test]
    fn reads_native_provider_keys_from_json() {
        let config: Config = serde_json::from_str(
            r#"{
                "provider": "openai",
                "openai_api_key": "sk-openai",
                "anthropic_api_key": "sk-ant",
                "gemini_api_key": "AIza"
            }"#,
        )
        .unwrap();
        assert_eq!(config.provider, "openai");
        assert_eq!(config.openai_api_key.as_deref(), Some("sk-openai"));
        assert_eq!(config.anthropic_api_key.as_deref(), Some("sk-ant"));
        assert_eq!(config.gemini_api_key.as_deref(), Some("AIza"));
        assert_eq!(
            config.api_key_for_provider("openai"),
            Some("sk-openai".to_string())
        );
        assert_eq!(
            config.api_key_for_provider("anthropic"),
            Some("sk-ant".to_string())
        );
        assert_eq!(
            config.api_key_for_provider("gemini"),
            Some("AIza".to_string())
        );
    }

    #[test]
    fn set_provider_adopts_the_stored_key() {
        let mut config: Config =
            serde_json::from_str("{\"gemini_api_key\": \"AIza\", \"provider\": \"gemini\"}").unwrap();
        config.set_provider("gemini").unwrap();
        assert_eq!(config.api_key.as_deref(), Some("AIza"));
        assert_eq!(
            config.base_url,
            "https://generativelanguage.googleapis.com/v1beta"
        );
    }
}
