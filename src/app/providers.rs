//! The `/provider` picker: listing the configured providers, switching the
//! active one, and updating the model list when the provider changes.

use crate::app::models::ModelCatalog;
use crate::app::{App, Overlay};

impl App {
    pub fn current_provider_index(&self) -> usize {
        crate::config::PROVIDERS
            .iter()
            .position(|p| p.id == self.config.provider)
            .unwrap_or(0)
    }

    pub fn open_providers(&mut self) {
        let selected = self.current_provider_index();
        self.overlay = Some(Overlay::Providers { selected });
    }

    pub fn move_provider_selection(&mut self, delta: i32) {
        let len = crate::config::PROVIDERS.len();
        if len == 0 {
            return;
        }
        if let Some(Overlay::Providers { selected }) = &mut self.overlay {
            let next = (*selected as i32 + delta).rem_euclid(len as i32);
            *selected = next as usize;
        }
    }

    pub fn apply_selected_provider(&mut self) {
        let Some(Overlay::Providers { selected }) = self.overlay else {
            return;
        };
        if let Some(provider) = crate::config::PROVIDERS.get(selected) {
            let id = provider.id;
            self.set_active_provider(id);
        }
        self.overlay = None;
    }

    pub fn set_active_provider(&mut self, provider_id: &str) {
        let Some(provider) = crate::config::find_provider(provider_id) else {
            self.push_error(format!("unknown provider: '{provider_id}'"));
            return;
        };
        if provider.id == self.config.provider {
            self.push_notice(format!("provider is already {}", provider.name));
            return;
        }
        if let Err(err) = self.config.set_provider(provider.id) {
            self.push_error(err);
            return;
        }
        // Invalidate cached model list from previous provider
        self.models = ModelCatalog::default();
        self.models_rx = None;
        self.models_fetch_auto = false;
        // APInex's default model is availability-based: resolve it against
        // the endpoint's live model list in the background (free models
        // first). Silent no-op without a key or for static-default providers.
        self.request_available_default();

        if self.config.api_key.is_some() {
            self.push_notice(format!(
                "switched provider to {} ({}) · model set to {}",
                provider.name, provider.base_url, self.config.model
            ));
        } else {
            self.push_notice(format!(
                "switched provider to {} ({}) — warning: no API key (set {} in config.json or env)",
                provider.name, provider.base_url, provider.env_key
            ));
        }
    }

    pub fn active_provider_name(&self) -> String {
        crate::config::find_provider(&self.config.provider)
            .map(|p| p.name.to_string())
            .unwrap_or_else(|| self.config.provider.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::config::Config;
    use crate::session::manager::SessionManager;

    fn test_app() -> App {
        App::new(Config::default(), SessionManager::for_tests())
    }

    #[test]
    fn provider_overlay_opens_and_navigates() {
        let mut app = test_app();
        app.open_providers();
        assert!(matches!(app.overlay, Some(Overlay::Providers { selected: 0 })));
        app.move_provider_selection(1);
        assert!(matches!(app.overlay, Some(Overlay::Providers { selected: 1 })));
        app.apply_selected_provider();
        assert_eq!(app.config.provider, "apinex");
        assert!(app.overlay.is_none());
    }
}
