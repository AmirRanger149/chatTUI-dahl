use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: u64,
    pub title: String,
    pub messages: Vec<Message>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    sessions: Vec<Session>,
    #[serde(default)]
    current: usize,
}

pub struct SessionManager {
    store: Store,
    path: PathBuf,
    persist: bool,
}

impl SessionManager {
    pub fn load() -> Result<Self> {
        let path = ProjectDirs::from("", "chatTUI", "chat-tui")
            .map(|d| d.data_dir().join("sessions.json"))
            .unwrap_or_else(|| PathBuf::from("sessions.json"));
        let mut manager = Self {
            store: Store::default(),
            path,
            persist: true,
        };
        if manager.path.exists() {
            let bytes = fs::read(&manager.path).with_context(|| {
                format!("reading session file {}", manager.path.display())
            })?;
            manager.store = serde_json::from_slice(&bytes).with_context(|| {
                format!(
                    "parsing session file {} (the file may be corrupted — move or delete it to start fresh)",
                    manager.path.display()
                )
            })?;
        }
        if manager.store.sessions.is_empty() {
            manager.insert_blank_session();
            // Creating the first empty session on startup. A failure here is
            // retried when the user actually produces content.
            let _ = manager.save();
        } else {
            manager.store.current = manager
                .store
                .current
                .min(manager.store.sessions.len() - 1);
        }
        Ok(manager)
    }

    pub fn save(&self) -> Result<()> {
        if !self.persist {
            return Ok(());
        }
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).with_context(|| {
                    format!("creating session directory {}", parent.display())
                })?;
            }
        }
        let bytes = serde_json::to_vec_pretty(&self.store).context("serializing sessions")?;
        atomic_write(&self.path, &bytes)
    }

    pub fn current(&self) -> &Session {
        self.store
            .sessions
            .get(self.store.current)
            .or_else(|| self.store.sessions.first())
            .expect("session store is never empty")
    }

    pub fn current_mut(&mut self) -> &mut Session {
        let index = self
            .store
            .current
            .min(self.store.sessions.len().saturating_sub(1));
        self.store
            .sessions
            .get_mut(index)
            .expect("session store is never empty")
    }

    pub fn sessions(&self) -> &[Session] {
        &self.store.sessions
    }

    pub fn current_index(&self) -> usize {
        self.store.current
    }

    pub fn len(&self) -> usize {
        self.store.sessions.len()
    }

    pub fn select(&mut self, index: usize) -> Result<()> {
        if index < self.store.sessions.len() {
            self.store.current = index;
            self.save()?;
        }
        Ok(())
    }

    pub fn delete_at(&mut self, index: usize) -> Result<()> {
        if self.store.sessions.len() <= 1 || index >= self.store.sessions.len() {
            return Ok(());
        }
        self.store.sessions.remove(index);
        if self.store.current == index {
            self.store.current = self.store.current.min(self.store.sessions.len() - 1);
        } else if self.store.current > index {
            self.store.current -= 1;
        }
        self.save()
    }

    pub fn new_session(&mut self) -> Result<()> {
        self.insert_blank_session();
        self.save()
    }

    pub fn add_message(&mut self, role: impl Into<String>, content: impl Into<String>) -> Result<()> {
        let message = Message {
            role: role.into(),
            content: content.into(),
        };
        let session = self.current_mut();
        if session.title == "New conversation" && message.role == "user" {
            if let Some(title) = title_from_content(&message.content) {
                session.title = title;
            }
        }
        session.messages.push(message);
        self.save()
    }

    fn insert_blank_session(&mut self) {
        let id = self
            .store
            .sessions
            .iter()
            .map(|session| session.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        self.store.sessions.insert(
            0,
            Session {
                id,
                title: "New conversation".into(),
                messages: Vec::new(),
            },
        );
        self.store.current = 0;
    }
}

fn title_from_content(content: &str) -> Option<String> {
    let title: String = content
        .lines()
        .find(|line| !line.trim().is_empty())?
        .chars()
        .take(42)
        .collect();
    if title.trim().is_empty() {
        None
    } else {
        Some(title)
    }
}

fn atomic_write(path: &PathBuf, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, bytes).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
impl SessionManager {
    pub fn for_tests() -> Self {
        let mut manager = Self {
            store: Store::default(),
            path: PathBuf::from("sessions.json"),
            persist: false,
        };
        manager.insert_blank_session();
        manager
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_sessions_get_unique_ids() {
        let mut manager = SessionManager::for_tests();
        manager.add_message("user", "hi").unwrap();
        manager.new_session().unwrap();
        manager.add_message("user", "there").unwrap();
        manager.new_session().unwrap();
        let ids: Vec<u64> = manager.sessions().iter().map(|session| session.id).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(ids.len(), sorted.len());
        assert_eq!(ids.len(), 3);
    }

    #[test]
    fn refuses_to_delete_the_last_session() {
        let mut manager = SessionManager::for_tests();
        assert_eq!(manager.len(), 1);
        manager.delete_at(0).unwrap();
        assert_eq!(manager.len(), 1);
    }

    #[test]
    fn delete_adjusts_current_index() {
        let mut manager = SessionManager::for_tests();
        manager.new_session().unwrap();
        manager.new_session().unwrap();
        manager.select(2).unwrap();
        manager.delete_at(0).unwrap();
        assert_eq!(manager.len(), 2);
        assert_eq!(manager.current_index(), 1);
    }

    #[test]
    fn empty_store_json_deserializes() {
        let store: Store = serde_json::from_str("{}").unwrap();
        assert!(store.sessions.is_empty());
        assert_eq!(store.current, 0);
    }

    #[test]
    fn title_uses_first_nonempty_line() {
        assert_eq!(title_from_content("\n  \nhello world"), Some("hello world".into()));
        assert_eq!(title_from_content("   \n"), None);
    }
}
