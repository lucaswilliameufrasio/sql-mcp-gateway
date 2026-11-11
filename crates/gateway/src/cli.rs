//! Command line surface. Client commands take an optional workspace: a saved
//! name or a gateway URL. Leaving it out opens a picker, so nothing has to be
//! memorized. `stdio` is the exception: its stdin/stdout carry MCP messages, so
//! it never prompts.

use std::io::IsTerminal;

use clap::{Parser, Subcommand};
use dialoguer::{Confirm, Input, Select, theme::ColorfulTheme};
use serde::Deserialize;
use url::Url;

use crate::{
    stdio_auth::{self, LoginStatus, StdioAuthError},
    workspace::{Workspace, is_valid_name, is_valid_slug},
    workspace_store::{self, StoreError, WorkspaceStore, slug_from_url},
};

const EXAMPLES: &str = "\
Examples:
  sql-mcp-gateway login                 Pick a workspace, or paste a gateway URL the first time
  sql-mcp-gateway login prod            Sign in to a saved workspace by name
  sql-mcp-gateway workspaces            See every workspace and whether you are signed in
  sql-mcp-gateway stdio prod            Command to configure in an MCP client

Run without a command to start the gateway server (operators only).";

#[derive(Parser)]
#[command(
    name = "sql-mcp-gateway",
    version,
    about = "SQL MCP Gateway: sign in to workspaces and bridge MCP clients to them",
    after_help = EXAMPLES,
    args_conflicts_with_subcommands = true
)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<CliCommand>,
    /// Earlier spelling of `stdio`, kept for existing MCP client configurations.
    #[arg(long = "stdio", hide = true, value_name = "URL", num_args = 0..=1)]
    legacy_stdio: Option<Option<String>>,
    #[arg(long = "login", hide = true, value_name = "URL")]
    legacy_login: Option<String>,
    #[arg(long = "logout", hide = true, value_name = "URL")]
    legacy_logout: Option<String>,
}

#[derive(Subcommand)]
enum CliCommand {
    /// Sign in to a workspace in the browser
    Login {
        /// Saved name or gateway URL; leave out to pick from a list
        workspace: Option<String>,
    },
    /// Sign out of a workspace, keeping it in your list
    Logout {
        /// Saved name or gateway URL; leave out to pick from a list
        workspace: Option<String>,
    },
    /// List saved workspaces and whether you are signed in
    #[command(visible_alias = "ls")]
    Workspaces,
    /// Sign out of a workspace and remove it from your list
    Remove {
        /// Saved name; leave out to pick from a list
        workspace: Option<String>,
    },
    /// Bridge an MCP client to a workspace over stdin/stdout
    Stdio {
        /// Saved name or gateway URL; optional when only one workspace is saved
        workspace: Option<String>,
    },
    /// Start the gateway HTTP server (operators; same as running without a command)
    Serve,
}

pub enum Command {
    Serve,
    Login(Option<String>),
    Logout(Option<String>),
    Workspaces,
    Remove(Option<String>),
    Stdio(Option<String>),
}

impl Cli {
    pub fn into_command(self) -> Command {
        if let Some(server) = self.legacy_stdio {
            return Command::Stdio(server);
        }
        if let Some(server) = self.legacy_login {
            return Command::Login(Some(server));
        }
        if let Some(server) = self.legacy_logout {
            return Command::Logout(Some(server));
        }
        match self.command {
            None | Some(CliCommand::Serve) => Command::Serve,
            Some(CliCommand::Login { workspace }) => Command::Login(workspace),
            Some(CliCommand::Logout { workspace }) => Command::Logout(workspace),
            Some(CliCommand::Workspaces) => Command::Workspaces,
            Some(CliCommand::Remove { workspace }) => Command::Remove(workspace),
            Some(CliCommand::Stdio { workspace }) => Command::Stdio(workspace),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error(transparent)]
    Auth(#[from] StdioAuthError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("{}", unknown_workspace_message(.name, .known))]
    UnknownWorkspace { name: String, known: Vec<String> },
    #[error("no workspace saved yet; run `sql-mcp-gateway login` and paste your gateway URL")]
    NoWorkspaces,
    #[error(
        "say which workspace to use, for example `sql-mcp-gateway {command} {example}` (saved: {})",
        .known.join(", ")
    )]
    WorkspaceRequired {
        command: &'static str,
        example: String,
        known: Vec<String>,
    },
    #[error(
        "the name `{0}` already belongs to another gateway; remove it first or choose a different name"
    )]
    NameTaken(String),
    #[error("cancelled")]
    Cancelled,
    #[error("the terminal prompt failed")]
    Prompt,
}

fn unknown_workspace_message(name: &str, known: &[String]) -> String {
    if known.is_empty() {
        format!("no workspace named `{name}`; run `sql-mcp-gateway login <gateway-url>` to add it")
    } else {
        format!(
            "no workspace named `{name}` (saved: {}); run `sql-mcp-gateway login <gateway-url>` to add another",
            known.join(", ")
        )
    }
}

/// What a command should act on: a saved workspace, or a URL not saved yet.
#[derive(Debug, PartialEq, Eq)]
enum Target {
    Saved(Workspace),
    NewUrl(Url),
}

impl Target {
    fn url(&self) -> &Url {
        match self {
            Self::Saved(workspace) => &workspace.url,
            Self::NewUrl(url) => url,
        }
    }
}

/// A reference containing `://` is a gateway URL; anything else is a saved name.
fn resolve(store: &WorkspaceStore, reference: &str) -> Result<Target, CliError> {
    let reference = reference.trim();
    if reference.contains("://") {
        let url = stdio_auth::normalize_server_url(reference)?;
        return Ok(match store.find_by_url(&url) {
            Some(workspace) => Target::Saved(workspace.clone()),
            None => Target::NewUrl(url),
        });
    }
    store
        .find(reference)
        .cloned()
        .map(Target::Saved)
        .ok_or_else(|| CliError::UnknownWorkspace {
            name: reference.to_owned(),
            known: saved_names(store),
        })
}

fn saved_names(store: &WorkspaceStore) -> Vec<String> {
    store
        .workspaces()
        .iter()
        .map(|workspace| workspace.slug.clone())
        .collect()
}

/// Picks the gateway for `stdio` without prompting: an explicit reference, then
/// `MCP_PUBLIC_URL`, then the only saved workspace.
fn resolve_stdio_server(
    store: Result<WorkspaceStore, StoreError>,
    reference: Option<&str>,
    environment_url: Option<&str>,
) -> Result<Url, CliError> {
    match reference {
        Some(reference) if reference.contains("://") => {
            Ok(stdio_auth::normalize_server_url(reference)?)
        }
        Some(reference) => Ok(resolve(&store?, reference)?.url().clone()),
        None => {
            if let Some(url) = environment_url {
                return Ok(stdio_auth::normalize_server_url(url)?);
            }
            let store = store?;
            match store.workspaces() {
                [] => Err(CliError::NoWorkspaces),
                [only] => Ok(only.url.clone()),
                [first, ..] => Err(CliError::WorkspaceRequired {
                    command: "stdio",
                    example: first.slug.clone(),
                    known: saved_names(&store),
                }),
            }
        }
    }
}

pub async fn stdio(reference: Option<String>) -> Result<(), CliError> {
    let environment_url = std::env::var("MCP_PUBLIC_URL")
        .ok()
        .filter(|url| !url.trim().is_empty());
    let store = workspace_store::default_path().and_then(|path| WorkspaceStore::load(&path));
    let server = resolve_stdio_server(store, reference.as_deref(), environment_url.as_deref())?;
    Ok(crate::stdio::run(server.as_str()).await?)
}

#[derive(Deserialize)]
struct AdvertisedWorkspaces {
    current_workspace: Option<String>,
    #[serde(default)]
    workspaces: Vec<Workspace>,
}

/// What a gateway says about itself and its siblings. The gateway is not
/// trusted to name other gateways: siblings are only ever saved after the
/// person confirms their URLs.
#[derive(Debug, Default, PartialEq, Eq)]
struct Discovery {
    own: Option<Workspace>,
    siblings: Vec<Workspace>,
}

fn interpret_advertisement(server: &Url, advertised: AdvertisedWorkspaces) -> Discovery {
    let mut discovery = Discovery::default();
    for mut workspace in advertised.workspaces {
        let Ok(url) = stdio_auth::normalize_server_url(workspace.url.as_str()) else {
            continue;
        };
        if !is_valid_slug(&workspace.slug) || !is_valid_name(&workspace.name) {
            continue;
        }
        if advertised.current_workspace.as_deref() == Some(workspace.slug.as_str()) {
            // Keep the address the person typed, not the one the gateway claims.
            workspace.url = server.clone();
            discovery.own = Some(workspace);
        } else if &url != server {
            workspace.url = url;
            discovery.siblings.push(workspace);
        }
    }
    discovery
}

async fn discover(server: &Url) -> Discovery {
    let Ok(client) = stdio_auth::oauth_http_client() else {
        return Discovery::default();
    };
    let Ok(endpoint) = server.join("v1/auth/config") else {
        return Discovery::default();
    };
    let Ok(response) = client.get(endpoint).send().await else {
        return Discovery::default();
    };
    match stdio_auth::bounded_json::<AdvertisedWorkspaces>(response).await {
        Ok(advertised) => interpret_advertisement(server, advertised),
        Err(()) => Discovery::default(),
    }
}

fn is_interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

fn describe(workspace: &Workspace) -> String {
    let mut description = workspace.slug.clone();
    if workspace.name != workspace.slug {
        description.push_str(&format!(" · {}", workspace.name));
    }
    if let Some(environment) = workspace.environment {
        description.push_str(&format!(" · {}", environment.as_str()));
    }
    description.push_str(&format!(
        "  {}",
        workspace.url.as_str().trim_end_matches('/')
    ));
    description
}

fn prompt_error(error: dialoguer::Error) -> CliError {
    let dialoguer::Error::IO(error) = error;
    if error.kind() == std::io::ErrorKind::Interrupted {
        CliError::Cancelled
    } else {
        CliError::Prompt
    }
}

fn pick_saved(store: &WorkspaceStore, prompt: &str) -> Result<Workspace, CliError> {
    let items = store.workspaces().iter().map(describe).collect::<Vec<_>>();
    let choice = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .items(&items)
        .default(0)
        .interact_opt()
        .map_err(prompt_error)?
        .ok_or(CliError::Cancelled)?;
    Ok(store.workspaces()[choice].clone())
}

fn ask_gateway_url() -> Result<Url, CliError> {
    let answer = Input::<String>::with_theme(&ColorfulTheme::default())
        .with_prompt("Gateway URL (copy it from the admin page \"Conectar MCP\")")
        .validate_with(|value: &String| {
            stdio_auth::normalize_server_url(value.trim())
                .map(|_| ())
                .map_err(
                    |_| "enter an https:// gateway address, for example https://sql.example.com",
                )
        })
        .interact_text()
        .map_err(prompt_error)?;
    Ok(stdio_auth::normalize_server_url(answer.trim())?)
}

/// Resolves the workspace for a command, opening a picker when none is named.
fn choose_saved(
    store: &WorkspaceStore,
    reference: Option<&str>,
    command: &'static str,
    prompt: &str,
) -> Result<Target, CliError> {
    if let Some(reference) = reference {
        return resolve(store, reference);
    }
    match store.workspaces() {
        [] => Err(CliError::NoWorkspaces),
        [only] => Ok(Target::Saved(only.clone())),
        [first, ..] if !is_interactive() => Err(CliError::WorkspaceRequired {
            command,
            example: first.slug.clone(),
            known: saved_names(store),
        }),
        _ => pick_saved(store, prompt).map(Target::Saved),
    }
}

fn choose_login_target(
    store: &WorkspaceStore,
    reference: Option<&str>,
) -> Result<Target, CliError> {
    if let Some(reference) = reference {
        return resolve(store, reference);
    }
    if !is_interactive() {
        return match store.workspaces() {
            [] => Err(CliError::NoWorkspaces),
            [only] => Ok(Target::Saved(only.clone())),
            [first, ..] => Err(CliError::WorkspaceRequired {
                command: "login",
                example: first.slug.clone(),
                known: saved_names(store),
            }),
        };
    }
    if store.is_empty() {
        eprintln!("No workspace saved yet. Paste the address of any of your gateways.");
        let url = ask_gateway_url()?;
        return Ok(match store.find_by_url(&url) {
            Some(workspace) => Target::Saved(workspace.clone()),
            None => Target::NewUrl(url),
        });
    }

    let mut items = store.workspaces().iter().map(describe).collect::<Vec<_>>();
    items.push("Add another gateway…".to_owned());
    let choice = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Sign in to which workspace?")
        .items(&items)
        .default(0)
        .interact_opt()
        .map_err(prompt_error)?
        .ok_or(CliError::Cancelled)?;
    match store.workspaces().get(choice) {
        Some(workspace) => Ok(Target::Saved(workspace.clone())),
        None => {
            let url = ask_gateway_url()?;
            Ok(match store.find_by_url(&url) {
                Some(workspace) => Target::Saved(workspace.clone()),
                None => Target::NewUrl(url),
            })
        }
    }
}

/// Names a gateway being saved for the first time: the name it advertises, or
/// one derived from its host that the person can change.
fn name_new_workspace(
    store: &WorkspaceStore,
    url: &Url,
    advertised: Option<Workspace>,
) -> Result<Workspace, CliError> {
    if let Some(mut workspace) = advertised {
        workspace.slug = store.available_slug(&workspace.slug, url);
        return Ok(workspace);
    }
    let suggestion = store.available_slug(&slug_from_url(url), url);
    let slug = if is_interactive() {
        Input::<String>::with_theme(&ColorfulTheme::default())
            .with_prompt("Short name for this workspace")
            .default(suggestion)
            .validate_with(|value: &String| {
                let value = value.trim();
                if !is_valid_slug(value) {
                    Err("use lowercase letters, digits and hyphens, for example prod")
                } else if store.find(value).is_some_and(|entry| &entry.url != url) {
                    Err("that name is already used by another workspace")
                } else {
                    Ok(())
                }
            })
            .interact_text()
            .map_err(prompt_error)?
            .trim()
            .to_owned()
    } else {
        suggestion
    };
    Ok(Workspace {
        name: slug.clone(),
        slug,
        environment: None,
        url: url.clone(),
    })
}

pub async fn login(reference: Option<String>) -> Result<(), CliError> {
    let path = workspace_store::default_path()?;
    let mut store = WorkspaceStore::load(&path)?;
    let target = choose_login_target(&store, reference.as_deref())?;
    let discovery = discover(target.url()).await;

    let workspace = match target {
        // A saved workspace keeps the name the person already uses.
        Target::Saved(saved) => match discovery.own {
            Some(advertised) => Workspace {
                name: advertised.name,
                environment: advertised.environment,
                ..saved
            },
            None => saved,
        },
        Target::NewUrl(url) => name_new_workspace(&store, &url, discovery.own)?,
    };

    stdio_auth::login(workspace.url.as_str()).await?;
    store
        .upsert(workspace.clone())
        .map_err(|_| CliError::NameTaken(workspace.slug.clone()))?;
    store.save(&path)?;
    println!("Signed in to {}.", describe(&workspace));

    let added = offer_siblings(&mut store, &path, discovery.siblings)?;
    let mut failures = 0_usize;
    if !added.is_empty() && confirm_sibling_login(added.len())? {
        for sibling in &added {
            eprintln!("\nSigning in to {}…", describe(sibling));
            match stdio_auth::login(sibling.url.as_str()).await {
                Ok(()) => println!("Signed in to {}.", describe(sibling)),
                Err(error) => {
                    failures += 1;
                    eprintln!(
                        "Could not sign in to {}: {error}. Try `sql-mcp-gateway login {}` later.",
                        sibling.slug, sibling.slug
                    );
                }
            }
        }
    }

    println!(
        "\nUse it from an MCP client with the command: sql-mcp-gateway stdio {}",
        workspace.slug
    );
    if store.workspaces().len() > 1 {
        println!("See every workspace with: sql-mcp-gateway workspaces");
    }
    if failures > 0 {
        return Err(CliError::Auth(StdioAuthError::Login));
    }
    Ok(())
}

/// Offers the sibling workspaces a gateway advertised and returns those saved.
fn offer_siblings(
    store: &mut WorkspaceStore,
    path: &std::path::Path,
    siblings: Vec<Workspace>,
) -> Result<Vec<Workspace>, CliError> {
    let unknown = siblings
        .into_iter()
        .filter(|sibling| store.find_by_url(&sibling.url).is_none())
        .map(|mut sibling| {
            sibling.slug = store.available_slug(&sibling.slug, &sibling.url);
            sibling
        })
        .collect::<Vec<_>>();
    if unknown.is_empty() {
        return Ok(Vec::new());
    }

    eprintln!("\nThis gateway lists other workspaces:");
    for sibling in &unknown {
        eprintln!("  {}", describe(sibling));
    }
    if !is_interactive() {
        eprintln!("Add one with `sql-mcp-gateway login <gateway-url>`.");
        return Ok(Vec::new());
    }
    let accepted = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Add them to your list? Check that the addresses are your company's")
        .default(true)
        .interact()
        .map_err(prompt_error)?;
    if !accepted {
        return Ok(Vec::new());
    }

    let mut added = Vec::new();
    for sibling in unknown {
        if store.upsert(sibling.clone()).is_ok() {
            added.push(sibling);
        }
    }
    store.save(path)?;
    Ok(added)
}

fn confirm_sibling_login(count: usize) -> Result<bool, CliError> {
    let prompt = if count == 1 {
        "Sign in to it now?".to_owned()
    } else {
        format!("Sign in to those {count} workspaces now?")
    };
    Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .default(true)
        .interact()
        .map_err(prompt_error)
}

pub async fn logout(reference: Option<String>) -> Result<(), CliError> {
    let target = match reference.as_deref() {
        // A URL signs out even when the workspace list is unavailable.
        Some(reference) if reference.contains("://") => {
            Target::NewUrl(stdio_auth::normalize_server_url(reference)?)
        }
        reference => {
            let store = WorkspaceStore::load(&workspace_store::default_path()?)?;
            choose_saved(&store, reference, "logout", "Sign out of which workspace?")?
        }
    };
    stdio_auth::logout(target.url().as_str()).await?;
    Ok(())
}

pub async fn remove(reference: Option<String>) -> Result<(), CliError> {
    let path = workspace_store::default_path()?;
    let mut store = WorkspaceStore::load(&path)?;
    let target = choose_saved(
        &store,
        reference.as_deref(),
        "remove",
        "Remove which workspace?",
    )?;
    let Target::Saved(workspace) = target else {
        return Err(CliError::UnknownWorkspace {
            name: reference.unwrap_or_default(),
            known: saved_names(&store),
        });
    };
    stdio_auth::logout(workspace.url.as_str()).await?;
    store.remove(&workspace.slug);
    store.save(&path)?;
    println!("Removed {} from your workspaces.", workspace.slug);
    Ok(())
}

pub async fn workspaces() -> Result<(), CliError> {
    let store = WorkspaceStore::load(&workspace_store::default_path()?)?;
    if store.is_empty() {
        println!("No workspace saved yet. Run `sql-mcp-gateway login` and paste your gateway URL.");
        return Ok(());
    }

    let mut rows = vec![[
        "NAME".to_owned(),
        "DESCRIPTION".to_owned(),
        "ENVIRONMENT".to_owned(),
        "GATEWAY".to_owned(),
        "SIGN-IN".to_owned(),
    ]];
    for workspace in store.workspaces() {
        let status = match stdio_auth::login_status(&workspace.url).await {
            LoginStatus::SignedIn => "signed in",
            LoginStatus::SignedOut => "signed out",
            LoginStatus::Unavailable => "credential store unavailable",
        };
        rows.push([
            workspace.slug.clone(),
            workspace.name.clone(),
            workspace
                .environment
                .map_or("-", |environment| environment.as_str())
                .to_owned(),
            workspace.url.as_str().trim_end_matches('/').to_owned(),
            status.to_owned(),
        ]);
    }
    print!("{}", format_table(&rows));
    println!("\nSign in:     sql-mcp-gateway login <name>");
    println!("MCP client:  sql-mcp-gateway stdio <name>");
    Ok(())
}

fn format_table<const COLUMNS: usize>(rows: &[[String; COLUMNS]]) -> String {
    let mut widths = [0_usize; COLUMNS];
    for row in rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    let mut table = String::new();
    for row in rows {
        let line = row
            .iter()
            .zip(widths)
            .map(|(cell, width)| {
                let padding = width - cell.chars().count();
                format!("{cell}{}", " ".repeat(padding))
            })
            .collect::<Vec<_>>()
            .join("  ");
        table.push_str(line.trim_end());
        table.push('\n');
    }
    table
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use url::Url;

    use super::{
        AdvertisedWorkspaces, Cli, CliError, Command, Target, format_table,
        interpret_advertisement, resolve, resolve_stdio_server,
    };
    use crate::{
        workspace::{Workspace, WorkspaceEnvironment},
        workspace_store::WorkspaceStore,
    };

    fn workspace(slug: &str, url: &str) -> Workspace {
        Workspace {
            slug: slug.to_owned(),
            name: slug.to_owned(),
            environment: None,
            url: Url::parse(url).expect("valid test URL"),
        }
    }

    fn store(entries: &[(&str, &str)]) -> WorkspaceStore {
        let mut store = WorkspaceStore::default();
        for (slug, url) in entries {
            store.upsert(workspace(slug, url)).expect("unique name");
        }
        store
    }

    fn parse(arguments: &[&str]) -> Command {
        Cli::try_parse_from(std::iter::once("sql-mcp-gateway").chain(arguments.iter().copied()))
            .expect("arguments are accepted")
            .into_command()
    }

    #[test]
    fn commands_take_an_optional_workspace_and_no_command_serves() {
        assert!(matches!(parse(&[]), Command::Serve));
        assert!(matches!(parse(&["serve"]), Command::Serve));
        assert!(matches!(parse(&["login"]), Command::Login(None)));
        assert!(matches!(parse(&["login", "prod"]), Command::Login(Some(name)) if name == "prod"));
        assert!(matches!(parse(&["logout"]), Command::Logout(None)));
        assert!(matches!(parse(&["workspaces"]), Command::Workspaces));
        assert!(matches!(parse(&["ls"]), Command::Workspaces));
        assert!(matches!(parse(&["remove", "dev"]), Command::Remove(Some(name)) if name == "dev"));
        assert!(matches!(parse(&["stdio", "prod"]), Command::Stdio(Some(name)) if name == "prod"));
        assert!(Cli::try_parse_from(["sql-mcp-gateway", "--unknown"]).is_err());
    }

    #[test]
    fn earlier_flag_spellings_keep_working_for_existing_client_configurations() {
        let url = "https://sql.example.test";
        assert!(matches!(parse(&["--stdio", url]), Command::Stdio(Some(server)) if server == url));
        assert!(matches!(parse(&["--stdio"]), Command::Stdio(None)));
        assert!(matches!(parse(&["--login", url]), Command::Login(Some(server)) if server == url));
        assert!(
            matches!(parse(&["--logout", url]), Command::Logout(Some(server)) if server == url)
        );
    }

    #[test]
    fn a_reference_is_a_saved_name_or_a_gateway_url() {
        let store = store(&[("prod", "https://sql.example.test/")]);

        assert_eq!(
            resolve(&store, "prod").expect("saved name"),
            Target::Saved(workspace("prod", "https://sql.example.test/"))
        );
        assert_eq!(
            resolve(&store, "https://sql.example.test/mcp").expect("saved URL"),
            Target::Saved(workspace("prod", "https://sql.example.test/"))
        );
        assert_eq!(
            resolve(&store, "https://sql-dev.example.test").expect("new URL"),
            Target::NewUrl(Url::parse("https://sql-dev.example.test/").expect("valid test URL"))
        );
        assert!(resolve(&store, "http://sql.example.test").is_err());

        let error = resolve(&store, "staging").expect_err("unknown name");
        assert!(matches!(&error, CliError::UnknownWorkspace { .. }));
        let message = error.to_string();
        assert!(message.contains("staging") && message.contains("prod"));
    }

    #[test]
    fn stdio_never_guesses_between_several_workspaces() {
        let several = || {
            Ok(store(&[
                ("prod", "https://sql.example.test/"),
                ("dev", "https://sql-dev.example.test/"),
            ]))
        };
        let url = |result: Result<Url, CliError>| result.expect("resolved").to_string();

        assert_eq!(
            url(resolve_stdio_server(several(), Some("dev"), None)),
            "https://sql-dev.example.test/"
        );
        assert_eq!(
            url(resolve_stdio_server(
                several(),
                None,
                Some("https://env.example.test")
            )),
            "https://env.example.test/"
        );
        assert_eq!(
            url(resolve_stdio_server(
                Ok(store(&[("prod", "https://sql.example.test/")])),
                None,
                None
            )),
            "https://sql.example.test/"
        );

        let ambiguous = resolve_stdio_server(several(), None, None).expect_err("ambiguous");
        assert!(ambiguous.to_string().contains("sql-mcp-gateway stdio prod"));
        assert!(matches!(
            resolve_stdio_server(Ok(WorkspaceStore::default()), None, None),
            Err(CliError::NoWorkspaces)
        ));
    }

    #[test]
    fn an_explicit_url_works_without_a_readable_workspace_list() {
        let unreadable = Err(crate::workspace_store::StoreError::NoConfigDirectory);
        let server = resolve_stdio_server(unreadable, Some("https://sql.example.test/mcp"), None)
            .expect("explicit URL needs no saved list");
        assert_eq!(server.as_str(), "https://sql.example.test/");
    }

    #[test]
    fn advertised_siblings_are_filtered_and_the_typed_address_is_kept() {
        let typed = Url::parse("https://internal-alias.example.test/").expect("valid test URL");
        let advertised = serde_json::from_value::<AdvertisedWorkspaces>(serde_json::json!({
            "issuer": "https://id.example.test",
            "current_workspace": "prod",
            "workspaces": [
                {"slug": "prod", "name": "Produção", "environment": "production", "url": "https://sql.example.test/"},
                {"slug": "dev", "name": "Dev", "url": "https://sql-dev.example.test/"},
                {"slug": "plain", "name": "Insecure", "url": "http://sql-plain.example.test/"},
                {"slug": "Bad Slug", "name": "Bad", "url": "https://sql-bad.example.test/"},
                {"slug": "path", "name": "Path", "url": "https://sql.example.test/phishing"}
            ]
        }))
        .expect("gateway response");

        let discovery = interpret_advertisement(&typed, advertised);

        let own = discovery.own.expect("current workspace");
        assert_eq!(own.slug, "prod");
        assert_eq!(own.environment, Some(WorkspaceEnvironment::Production));
        assert_eq!(own.url, typed);
        assert_eq!(
            discovery.siblings,
            vec![Workspace {
                slug: "dev".to_owned(),
                name: "Dev".to_owned(),
                environment: None,
                url: Url::parse("https://sql-dev.example.test/").expect("valid test URL"),
            }]
        );
    }

    #[test]
    fn gateways_without_workspaces_advertise_nothing() {
        let server = Url::parse("https://sql.example.test/").expect("valid test URL");
        let advertised = serde_json::from_value::<AdvertisedWorkspaces>(
            serde_json::json!({"issuer": "https://id.example.test"}),
        )
        .expect("gateway response");
        let discovery = interpret_advertisement(&server, advertised);
        assert!(discovery.own.is_none() && discovery.siblings.is_empty());
    }

    #[test]
    fn tables_align_columns_by_character_count() {
        let table = format_table(&[
            ["NAME".to_owned(), "DESCRIPTION".to_owned()],
            ["prod".to_owned(), "Produção".to_owned()],
            ["staging".to_owned(), "x".to_owned()],
        ]);
        assert_eq!(
            table,
            "NAME     DESCRIPTION\nprod     Produção\nstaging  x\n"
        );
    }
}
