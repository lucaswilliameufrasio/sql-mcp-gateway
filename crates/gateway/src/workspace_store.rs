//! The workspaces a person saved on this machine, so commands accept a short
//! name instead of a gateway URL. The file holds names and URLs only; tokens
//! stay in the operating system credential store.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use url::Url;

use crate::workspace::{Workspace, is_valid_name, is_valid_slug};

const CONFIG_DIRECTORY_VARIABLE: &str = "SQL_MCP_CONFIG_DIR";
const FILE_NAME: &str = "workspaces.json";

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("could not find a configuration directory; set {CONFIG_DIRECTORY_VARIABLE}")]
    NoConfigDirectory,
    #[error("could not read {0}")]
    Read(PathBuf),
    #[error("{0} is not a valid workspace list; fix or delete it and sign in again")]
    Corrupt(PathBuf),
    #[error("could not write {0}")]
    Write(PathBuf),
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceStore {
    #[serde(default)]
    workspaces: Vec<Workspace>,
}

/// Another saved workspace already uses the requested name.
#[derive(Debug, PartialEq, Eq)]
pub struct NameTaken;

pub fn default_path() -> Result<PathBuf, StoreError> {
    let directory = match std::env::var_os(CONFIG_DIRECTORY_VARIABLE) {
        Some(directory) if !directory.is_empty() => PathBuf::from(directory),
        _ => dirs::config_dir()
            .ok_or(StoreError::NoConfigDirectory)?
            .join("sql-mcp-gateway"),
    };
    Ok(directory.join(FILE_NAME))
}

impl WorkspaceStore {
    pub fn load(path: &Path) -> Result<Self, StoreError> {
        let contents = match std::fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(_) => return Err(StoreError::Read(path.to_owned())),
        };
        let store = serde_json::from_str::<Self>(&contents)
            .map_err(|_| StoreError::Corrupt(path.to_owned()))?;
        let valid = store.workspaces.iter().enumerate().all(|(index, entry)| {
            is_valid_slug(&entry.slug)
                && is_valid_name(&entry.name)
                && !store.workspaces[..index]
                    .iter()
                    .any(|earlier| earlier.slug == entry.slug || earlier.url == entry.url)
        });
        if !valid {
            return Err(StoreError::Corrupt(path.to_owned()));
        }
        Ok(store)
    }

    /// Replaces the file atomically so a concurrent reader never sees a partial list.
    pub fn save(&self, path: &Path) -> Result<(), StoreError> {
        let failed = || StoreError::Write(path.to_owned());
        let directory = path.parent().ok_or_else(failed)?;
        std::fs::create_dir_all(directory).map_err(|_| failed())?;
        let serialized = serde_json::to_string_pretty(self).map_err(|_| failed())?;
        let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
        std::fs::write(&temporary, format!("{serialized}\n")).map_err(|_| failed())?;
        std::fs::rename(&temporary, path).map_err(|_| {
            let _ = std::fs::remove_file(&temporary);
            failed()
        })
    }

    pub fn workspaces(&self) -> &[Workspace] {
        &self.workspaces
    }

    pub fn is_empty(&self) -> bool {
        self.workspaces.is_empty()
    }

    pub fn find(&self, slug: &str) -> Option<&Workspace> {
        self.workspaces.iter().find(|entry| entry.slug == slug)
    }

    pub fn find_by_url(&self, url: &Url) -> Option<&Workspace> {
        self.workspaces.iter().find(|entry| &entry.url == url)
    }

    /// Saves a workspace. A gateway URL keeps a single entry, so saving a known
    /// URL renames it; a name never moves silently to a different gateway.
    pub fn upsert(&mut self, workspace: Workspace) -> Result<(), NameTaken> {
        if self
            .workspaces
            .iter()
            .any(|entry| entry.slug == workspace.slug && entry.url != workspace.url)
        {
            return Err(NameTaken);
        }
        match self
            .workspaces
            .iter_mut()
            .find(|entry| entry.url == workspace.url)
        {
            Some(entry) => *entry = workspace,
            None => self.workspaces.push(workspace),
        }
        Ok(())
    }

    pub fn remove(&mut self, slug: &str) -> Option<Workspace> {
        let index = self
            .workspaces
            .iter()
            .position(|entry| entry.slug == slug)?;
        Some(self.workspaces.remove(index))
    }

    /// Returns `preferred`, or the first `preferred-N` no other gateway uses.
    pub fn available_slug(&self, preferred: &str, url: &Url) -> String {
        let taken = |slug: &str| {
            self.workspaces
                .iter()
                .any(|entry| entry.slug == slug && &entry.url != url)
        };
        if !taken(preferred) {
            return preferred.to_owned();
        }
        (2..)
            .map(|suffix| format!("{preferred}-{suffix}"))
            .find(|candidate| !taken(candidate))
            .expect("an unused numeric suffix always exists")
    }
}

/// Suggests a name from the gateway host: its first DNS label, or `local`.
pub fn slug_from_url(url: &Url) -> String {
    let label = match url.host() {
        Some(url::Host::Domain(domain)) if !domain.eq_ignore_ascii_case("localhost") => {
            domain.split('.').next().unwrap_or_default()
        }
        _ => "local",
    };
    let slug = label
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_lowercase() || character.is_ascii_digit() {
                character
            } else {
                '-'
            }
        })
        .take(32)
        .collect::<String>();
    let slug = slug.trim_matches('-');
    if is_valid_slug(slug) {
        slug.to_owned()
    } else {
        "gateway".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use url::Url;

    use super::{NameTaken, StoreError, WorkspaceStore, slug_from_url};
    use crate::workspace::{Workspace, WorkspaceEnvironment};

    fn workspace(slug: &str, url: &str) -> Workspace {
        Workspace {
            slug: slug.to_owned(),
            name: slug.to_owned(),
            environment: Some(WorkspaceEnvironment::Staging),
            url: Url::parse(url).expect("valid test URL"),
        }
    }

    fn temporary_file(test: &str) -> std::path::PathBuf {
        std::env::temp_dir()
            .join(format!(
                "sql-mcp-gateway-{test}-{}-{}",
                std::process::id(),
                uuid::Uuid::new_v4()
            ))
            .join("workspaces.json")
    }

    #[test]
    fn a_missing_file_is_an_empty_list_and_saved_workspaces_round_trip() {
        let path = temporary_file("round-trip");
        let mut store = WorkspaceStore::load(&path).expect("missing file is empty");
        assert!(store.is_empty());

        store
            .upsert(workspace("prod", "https://sql.example.test/"))
            .expect("new name");
        store.save(&path).expect("writable temporary directory");

        let reloaded = WorkspaceStore::load(&path).expect("saved file loads");
        assert_eq!(reloaded, store);
        assert_eq!(
            reloaded.find("prod").map(|entry| entry.url.as_str()),
            Some("https://sql.example.test/")
        );
        let contents = std::fs::read_to_string(&path).expect("saved file");
        assert!(!contents.to_lowercase().contains("token"));
        std::fs::remove_dir_all(path.parent().expect("parent")).expect("cleanup");
    }

    #[test]
    fn corrupt_or_ambiguous_files_are_reported_instead_of_guessed() {
        for contents in [
            "not json",
            r#"{"workspaces":[{"slug":"Bad Name","name":"x","url":"https://a.example.test/"}]}"#,
            r#"{"workspaces":[{"slug":"a","name":"a","url":"https://a.example.test/"},{"slug":"a","name":"a","url":"https://b.example.test/"}]}"#,
        ] {
            let path = temporary_file("corrupt");
            std::fs::create_dir_all(path.parent().expect("parent")).expect("temporary directory");
            std::fs::write(&path, contents).expect("temporary file");
            assert!(matches!(
                WorkspaceStore::load(&path),
                Err(StoreError::Corrupt(_))
            ));
            std::fs::remove_dir_all(path.parent().expect("parent")).expect("cleanup");
        }
    }

    #[test]
    fn a_name_never_moves_to_another_gateway_but_a_gateway_can_be_renamed() {
        let mut store = WorkspaceStore::default();
        store
            .upsert(workspace("prod", "https://sql.example.test/"))
            .expect("new name");

        assert_eq!(
            store.upsert(workspace("prod", "https://attacker.example.test/")),
            Err(NameTaken)
        );
        assert_eq!(
            store.find("prod").map(|entry| entry.url.as_str()),
            Some("https://sql.example.test/")
        );

        store
            .upsert(workspace("production", "https://sql.example.test/"))
            .expect("same gateway may be renamed");
        assert_eq!(store.workspaces().len(), 1);
        assert!(store.find("prod").is_none());
        assert!(store.find("production").is_some());

        assert!(store.remove("production").is_some());
        assert!(store.remove("production").is_none());
    }

    #[test]
    fn suggested_names_come_from_the_host_and_avoid_collisions() {
        let name = |url: &str| slug_from_url(&Url::parse(url).expect("valid test URL"));
        assert_eq!(name("https://sql-dev.example.test/"), "sql-dev");
        assert_eq!(name("https://SQL_Stg.example.test/"), "sql-stg");
        assert_eq!(name("http://127.0.0.1:8080/"), "local");
        assert_eq!(name("http://localhost:8080/"), "local");

        let mut store = WorkspaceStore::default();
        let first = Url::parse("https://sql.example.test/").expect("valid test URL");
        let second = Url::parse("https://sql.other.test/").expect("valid test URL");
        store
            .upsert(workspace("sql", first.as_str()))
            .expect("new name");
        assert_eq!(store.available_slug("sql", &first), "sql");
        assert_eq!(store.available_slug("sql", &second), "sql-2");
    }
}
