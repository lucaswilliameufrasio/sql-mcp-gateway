//! A workspace is one gateway deployment (typically one environment).
//!
//! Workspaces stay isolated: each has its own control database, identity
//! configuration and Vault. A deployment may only *advertise* its siblings so
//! the admin UI and the command line can offer a switcher. Nothing here grants
//! access across workspaces.

use serde::{Deserialize, Serialize};
use url::Url;

pub const MAX_WORKSPACES: usize = 16;
const MAX_SLUG_LENGTH: usize = 32;
const MAX_NAME_LENGTH: usize = 48;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceEnvironment {
    Production,
    Staging,
    Development,
}

impl WorkspaceEnvironment {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Production => "production",
            Self::Staging => "staging",
            Self::Development => "development",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Workspace {
    pub slug: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<WorkspaceEnvironment>,
    /// Public origin of the gateway, which also serves the admin UI.
    pub url: Url,
}

/// The workspaces a deployment advertises, including itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceDirectory {
    pub current: String,
    pub workspaces: Vec<Workspace>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceConfigError {
    /// Only one of `WORKSPACE_SLUG` and `WORKSPACES` was provided.
    Incomplete,
    /// `WORKSPACES` is not a JSON list of valid, unique workspaces.
    InvalidList,
    /// `WORKSPACE_SLUG` names no entry of `WORKSPACES`.
    UnknownCurrent,
    /// The current entry's URL differs from `MCP_PUBLIC_URL`.
    CurrentUrlMismatch,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWorkspace {
    slug: String,
    name: Option<String>,
    environment: Option<WorkspaceEnvironment>,
    url: String,
}

impl WorkspaceDirectory {
    /// Builds the directory from `WORKSPACE_SLUG` and the `WORKSPACES` JSON list.
    /// Both absent means the deployment does not advertise workspaces.
    pub fn from_settings(
        current: Option<&str>,
        list: Option<&str>,
        public_url: &Url,
    ) -> Result<Option<Self>, WorkspaceConfigError> {
        let (current, list) = match (current, list) {
            (None, None) => return Ok(None),
            (Some(current), Some(list)) => (current.trim(), list),
            _ => return Err(WorkspaceConfigError::Incomplete),
        };

        let raw = serde_json::from_str::<Vec<RawWorkspace>>(list)
            .map_err(|_| WorkspaceConfigError::InvalidList)?;
        if raw.is_empty() || raw.len() > MAX_WORKSPACES {
            return Err(WorkspaceConfigError::InvalidList);
        }

        let mut workspaces = Vec::<Workspace>::with_capacity(raw.len());
        for entry in raw {
            let name = entry.name.unwrap_or_else(|| entry.slug.clone());
            let name = name.trim().to_owned();
            let url = parse_origin(&entry.url).ok_or(WorkspaceConfigError::InvalidList)?;
            if !is_valid_slug(&entry.slug)
                || !is_valid_name(&name)
                || workspaces
                    .iter()
                    .any(|known| known.slug == entry.slug || known.url == url)
            {
                return Err(WorkspaceConfigError::InvalidList);
            }
            workspaces.push(Workspace {
                slug: entry.slug,
                name,
                environment: entry.environment,
                url,
            });
        }

        let own = workspaces
            .iter()
            .find(|workspace| workspace.slug == current)
            .ok_or(WorkspaceConfigError::UnknownCurrent)?;
        if own.url.origin() != public_url.origin() {
            return Err(WorkspaceConfigError::CurrentUrlMismatch);
        }

        Ok(Some(Self {
            current: current.to_owned(),
            workspaces,
        }))
    }
}

/// Lowercase ASCII letters, digits and inner hyphens: safe in MCP server names,
/// shell arguments and file names without quoting.
pub fn is_valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= MAX_SLUG_LENGTH
        && !slug.starts_with('-')
        && !slug.ends_with('-')
        && slug
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= MAX_NAME_LENGTH
        && !name.chars().any(char::is_control)
}

/// Accepts an HTTPS origin (HTTP only on loopback) with no path, query,
/// fragment or credentials.
fn parse_origin(value: &str) -> Option<Url> {
    let url = Url::parse(value).ok()?;
    let secure = url.scheme() == "https"
        || (url.scheme() == "http" && crate::config::is_loopback_host(url.host_str()));
    (secure
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.path() == "/")
        .then_some(url)
}

#[cfg(test)]
mod tests {
    use url::Url;

    use super::{WorkspaceConfigError, WorkspaceDirectory, WorkspaceEnvironment, is_valid_slug};

    const LIST: &str = r#"[
        {"slug":"dev","name":"Desenvolvimento","environment":"development","url":"https://sql-dev.example.test"},
        {"slug":"staging","url":"https://sql-staging.example.test/"},
        {"slug":"prod","name":"Produção","environment":"production","url":"https://sql.example.test"}
    ]"#;

    fn public_url(value: &str) -> Url {
        Url::parse(value).expect("valid test URL")
    }

    #[test]
    fn deployments_without_workspace_settings_advertise_nothing() {
        let directory =
            WorkspaceDirectory::from_settings(None, None, &public_url("https://sql.example.test"));
        assert_eq!(directory, Ok(None));
    }

    #[test]
    fn the_same_list_identifies_each_deployment_by_its_slug() {
        let directory = WorkspaceDirectory::from_settings(
            Some("prod"),
            Some(LIST),
            &public_url("https://sql.example.test"),
        )
        .expect("valid settings")
        .expect("workspaces are advertised");

        assert_eq!(directory.current, "prod");
        assert_eq!(directory.workspaces.len(), 3);
        assert_eq!(directory.workspaces[1].name, "staging");
        assert_eq!(directory.workspaces[1].environment, None);
        assert_eq!(
            directory.workspaces[2].environment,
            Some(WorkspaceEnvironment::Production)
        );
    }

    #[test]
    fn a_deployment_cannot_claim_another_workspace_identity() {
        let staging_claiming_production = WorkspaceDirectory::from_settings(
            Some("prod"),
            Some(LIST),
            &public_url("https://sql-staging.example.test"),
        );
        assert_eq!(
            staging_claiming_production,
            Err(WorkspaceConfigError::CurrentUrlMismatch)
        );

        let unknown = WorkspaceDirectory::from_settings(
            Some("qa"),
            Some(LIST),
            &public_url("https://sql.example.test"),
        );
        assert_eq!(unknown, Err(WorkspaceConfigError::UnknownCurrent));
    }

    #[test]
    fn partial_or_malformed_settings_are_rejected() {
        let url = public_url("https://sql.example.test");
        assert_eq!(
            WorkspaceDirectory::from_settings(Some("prod"), None, &url),
            Err(WorkspaceConfigError::Incomplete)
        );
        assert_eq!(
            WorkspaceDirectory::from_settings(None, Some(LIST), &url),
            Err(WorkspaceConfigError::Incomplete)
        );
        for list in [
            "not json",
            "[]",
            r#"[{"slug":"Prod","url":"https://sql.example.test"}]"#,
            r#"[{"slug":"prod","url":"http://sql.example.test"}]"#,
            r#"[{"slug":"prod","url":"https://sql.example.test/admin"}]"#,
            r#"[{"slug":"prod","url":"https://user@sql.example.test"}]"#,
            r#"[{"slug":"prod","environment":"qa","url":"https://sql.example.test"}]"#,
            r#"[{"slug":"prod","url":"https://sql.example.test"},{"slug":"prod","url":"https://other.example.test"}]"#,
            r#"[{"slug":"prod","url":"https://sql.example.test"},{"slug":"copy","url":"https://sql.example.test"}]"#,
        ] {
            assert_eq!(
                WorkspaceDirectory::from_settings(Some("prod"), Some(list), &url),
                Err(WorkspaceConfigError::InvalidList),
                "{list}"
            );
        }
    }

    #[test]
    fn slugs_are_safe_identifiers() {
        for slug in ["prod", "staging-2", "a"] {
            assert!(is_valid_slug(slug), "{slug}");
        }
        for slug in [
            "",
            "Prod",
            "-prod",
            "prod-",
            "pro d",
            "produção",
            "a/b",
            &"a".repeat(33),
        ] {
            assert!(!is_valid_slug(slug), "{slug}");
        }
    }
}
