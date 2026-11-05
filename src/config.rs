use crate::protocol::Request;
use anyhow::{bail, Context, Result};
use iroh::{EndpointId, RelayUrl};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

const SYSTEM_CONFIG: &str = "/etc/clipsy/config.toml";

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub relay_url: Option<RelayUrl>,
    pub key_file: Option<PathBuf>,
    default_server: Option<String>,
    #[serde(default)]
    servers: BTreeMap<String, EndpointId>,
    #[serde(default)]
    clients: BTreeMap<String, Client>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Client {
    pub id: EndpointId,
    #[serde(default = "allowed")]
    pub read: bool,
    #[serde(default = "allowed")]
    pub write: bool,
}

fn allowed() -> bool {
    true
}

impl Client {
    pub fn allows(&self, request: &Request) -> bool {
        match request {
            Request::Read => self.read,
            Request::Write { .. } => self.write,
        }
    }
}

impl Config {
    pub fn parse(text: &str) -> Result<Self> {
        let config: Config = toml::from_str(text)?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        if let Some(name) = &self.default_server {
            if !self.servers.contains_key(name) {
                bail!("default_server `{name}` is not listed under [servers]");
            }
        }
        if let Some(path) = self.key_file.as_ref().filter(|p| !p.is_absolute()) {
            bail!("key_file must be an absolute path, got {}", path.display());
        }
        let mut seen = BTreeMap::new();
        for (name, client) in &self.clients {
            if let Some(other) = seen.insert(client.id, name) {
                bail!("clients `{other}` and `{name}` have the same id");
            }
        }
        Ok(())
    }

    pub fn load(path: Option<&Path>) -> Result<Self> {
        let Some(path) = path else {
            return Ok(Self::default());
        };
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn discover(env: impl Fn(&str) -> Option<OsString>) -> Result<Self> {
        let path = first_existing(candidate_config_paths(env))?;
        Self::load(path.as_deref())
    }

    pub fn client(&self, id: &EndpointId) -> Option<(&str, &Client)> {
        self.clients
            .iter()
            .find(|(_, client)| client.id == *id)
            .map(|(name, client)| (name.as_str(), client))
    }

    pub fn resolve_server(&self, choice: Option<&str>) -> Result<EndpointId> {
        let name = match (choice.filter(|c| !c.is_empty()), &self.default_server) {
            (Some(choice), _) => choice,
            (None, Some(default)) => default,
            (None, None) => return self.only_server(),
        };
        if let Some(id) = self.servers.get(name) {
            return Ok(*id);
        }
        name.parse().map_err(|_| {
            anyhow::anyhow!(
                "unknown server `{name}`; known servers: {}",
                self.server_names()
            )
        })
    }

    fn only_server(&self) -> Result<EndpointId> {
        let mut ids = self.servers.values();
        match (ids.next(), ids.next()) {
            (Some(id), None) => Ok(*id),
            (None, _) => bail!("no server configured; pass --server or set CLIPSY_SERVER"),
            _ => bail!(
                "several servers configured ({}); pass --server, set CLIPSY_SERVER or set default_server",
                self.server_names()
            ),
        }
    }

    fn server_names(&self) -> String {
        let names: Vec<_> = self.servers.keys().map(String::as_str).collect();
        if names.is_empty() {
            "none".into()
        } else {
            names.join(", ")
        }
    }
}

fn candidate_config_paths(env: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(dir) = xdg_dir(&env, "XDG_CONFIG_HOME", ".config") {
        paths.push(dir.join("config.toml"));
    }
    paths.push(PathBuf::from(SYSTEM_CONFIG));
    paths
}

fn first_existing(paths: impl IntoIterator<Item = PathBuf>) -> Result<Option<PathBuf>> {
    for path in paths {
        if path
            .try_exists()
            .with_context(|| format!("checking {}", path.display()))?
        {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

pub fn default_key_path(env: impl Fn(&str) -> Option<OsString>) -> Result<PathBuf> {
    Ok(xdg_dir(&env, "XDG_STATE_HOME", ".local/state")?.join("secret.key"))
}

pub fn process_env(name: &str) -> Option<OsString> {
    std::env::var_os(name)
}

fn xdg_dir(
    env: &impl Fn(&str) -> Option<OsString>,
    variable: &str,
    home_fallback: &str,
) -> Result<PathBuf> {
    let base = match env(variable).map(PathBuf::from).filter(|d| d.is_absolute()) {
        Some(dir) => dir,
        None => PathBuf::from(
            env("HOME")
                .filter(|home| !home.is_empty())
                .context("HOME is not set")?,
        )
        .join(home_fallback),
    };
    Ok(base.join("clipsy"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn id(seed: u8) -> EndpointId {
        SecretKey::from_bytes(&[seed; 32]).public()
    }

    fn laptop() -> EndpointId {
        id(1)
    }

    fn desktop() -> EndpointId {
        id(2)
    }

    fn devbox() -> EndpointId {
        id(3)
    }

    fn sandbox() -> EndpointId {
        id(4)
    }

    fn nix_rendered_config() -> String {
        format!(
            r#"
default_server = "laptop"
key_file = "/run/secrets/clipsy-key"
relay_url = "https://relay.example.com"

[clients.devbox]
id = "{devbox}"
read = true
write = true

[clients.sandbox]
id = "{sandbox}"
read = false
write = true

[servers]
laptop = "{laptop}"
desktop = "{desktop}"
"#,
            devbox = devbox(),
            sandbox = sandbox(),
            laptop = laptop(),
            desktop = desktop(),
        )
    }

    fn servers_only(default: Option<&str>, servers: &[(&str, EndpointId)]) -> Config {
        let mut text = default
            .map(|name| format!("default_server = \"{name}\"\n"))
            .unwrap_or_default();
        text.push_str("[servers]\n");
        for (name, id) in servers {
            text.push_str(&format!("{name} = \"{id}\"\n"));
        }
        Config::parse(&text).unwrap()
    }

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let vars: HashMap<String, OsString> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), OsString::from(v)))
            .collect();
        move |name| vars.get(name).cloned()
    }

    #[test]
    fn parses_nix_rendered_config() {
        let config = Config::parse(&nix_rendered_config()).unwrap();

        assert_eq!(
            config.relay_url,
            Some("https://relay.example.com".parse().unwrap())
        );
        assert_eq!(
            config.key_file,
            Some(PathBuf::from("/run/secrets/clipsy-key"))
        );
        assert_eq!(config.resolve_server(None).unwrap(), laptop());
        assert_eq!(config.client(&devbox()).unwrap().0, "devbox");
        assert!(!config.client(&sandbox()).unwrap().1.read);
    }

    #[test]
    fn empty_config_is_valid() {
        let config = Config::parse("").unwrap();

        assert_eq!(config.relay_url, None);
        assert_eq!(config.key_file, None);
        assert!(config.client(&devbox()).is_none());
    }

    #[test]
    fn missing_config_file_means_defaults() {
        assert!(Config::load(None).unwrap().client(&devbox()).is_none());
    }

    #[test]
    fn loads_config_from_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, nix_rendered_config()).unwrap();

        let config = Config::load(Some(&path)).unwrap();

        assert_eq!(config.resolve_server(Some("desktop")).unwrap(), desktop());
    }

    #[test]
    fn unreadable_config_file_names_the_path() {
        let err = Config::load(Some(Path::new("/nonexistent/clipsy.toml"))).unwrap_err();

        assert!(
            err.to_string().contains("/nonexistent/clipsy.toml"),
            "{err}"
        );
    }

    #[test]
    fn permissions_default_to_allowed() {
        let config = Config::parse(&format!("[clients.devbox]\nid = \"{}\"\n", devbox())).unwrap();

        let (_, client) = config.client(&devbox()).unwrap();

        assert!(client.read && client.write);
    }

    #[test]
    fn client_permissions_gate_each_request_kind() {
        let config = Config::parse(&nix_rendered_config()).unwrap();
        let read = Request::Read;
        let write = Request::Write {
            content: "ls -la\n".into(),
        };

        let (_, devbox) = config.client(&devbox()).unwrap();
        let (_, sandbox) = config.client(&sandbox()).unwrap();

        assert!(devbox.allows(&read) && devbox.allows(&write));
        assert!(!sandbox.allows(&read) && sandbox.allows(&write));
    }

    #[test]
    fn unknown_node_is_not_a_client() {
        let config = Config::parse(&nix_rendered_config()).unwrap();

        assert!(config.client(&laptop()).is_none());
    }

    #[test]
    fn rejects_unknown_fields() {
        let err = Config::parse("relay = \"https://example.com\"").unwrap_err();

        assert!(format!("{err:#}").contains("unknown field"), "{err:#}");
    }

    #[test]
    fn rejects_malformed_node_id() {
        assert!(Config::parse("[servers]\nlaptop = \"not-a-node-id\"").is_err());
    }

    #[test]
    fn rejects_default_server_that_is_not_listed() {
        let text = format!(
            "default_server = \"macbook\"\n[servers]\nlaptop = \"{}\"",
            laptop()
        );

        let err = Config::parse(&text).unwrap_err();

        assert!(err.to_string().contains("macbook"), "{err}");
    }

    #[test]
    fn explicit_choice_beats_default_server() {
        let config = servers_only(
            Some("laptop"),
            &[("laptop", laptop()), ("desktop", desktop())],
        );

        assert_eq!(config.resolve_server(Some("desktop")).unwrap(), desktop());
    }

    #[test]
    fn explicit_choice_can_be_a_raw_node_id() {
        let config = Config::default();

        let chosen = config.resolve_server(Some(&desktop().to_string())).unwrap();

        assert_eq!(chosen, desktop());
    }

    #[test]
    fn unknown_server_name_lists_known_servers() {
        let config = servers_only(None, &[("laptop", laptop()), ("desktop", desktop())]);

        let err = config.resolve_server(Some("macbook")).unwrap_err();

        assert!(err.to_string().contains("desktop, laptop"), "{err}");
    }

    #[test]
    fn single_server_needs_no_default() {
        let config = servers_only(None, &[("laptop", laptop())]);

        assert_eq!(config.resolve_server(None).unwrap(), laptop());
    }

    #[test]
    fn several_servers_without_default_is_ambiguous() {
        let config = servers_only(None, &[("laptop", laptop()), ("desktop", desktop())]);

        let err = config.resolve_server(None).unwrap_err();

        assert!(err.to_string().contains("CLIPSY_SERVER"), "{err}");
    }

    #[test]
    fn no_servers_is_an_error() {
        let err = Config::default().resolve_server(None).unwrap_err();

        assert!(err.to_string().contains("no server configured"), "{err}");
    }

    #[test]
    fn user_config_is_preferred_over_system_config() {
        let paths = candidate_config_paths(env(&[("HOME", "/Users/alice")]));

        assert_eq!(
            paths,
            [
                PathBuf::from("/Users/alice/.config/clipsy/config.toml"),
                PathBuf::from("/etc/clipsy/config.toml"),
            ]
        );
    }

    #[test]
    fn xdg_config_home_overrides_home() {
        let paths = candidate_config_paths(env(&[
            ("HOME", "/home/alice"),
            ("XDG_CONFIG_HOME", "/home/alice/dotfiles"),
        ]));

        assert_eq!(
            paths[0],
            PathBuf::from("/home/alice/dotfiles/clipsy/config.toml")
        );
    }

    #[test]
    fn without_home_only_system_config_is_considered() {
        assert_eq!(
            candidate_config_paths(env(&[])),
            [PathBuf::from("/etc/clipsy/config.toml")]
        );
    }

    #[test]
    fn first_existing_skips_missing_paths() {
        let dir = TempDir::new().unwrap();
        let system = dir.path().join("system.toml");
        std::fs::write(&system, "").unwrap();

        let found = first_existing([dir.path().join("user.toml"), system.clone()]).unwrap();

        assert_eq!(found, Some(system));
    }

    #[test]
    fn first_existing_finds_nothing_when_no_file_exists() {
        let dir = TempDir::new().unwrap();

        assert_eq!(
            first_existing([dir.path().join("config.toml")]).unwrap(),
            None
        );
    }

    #[test]
    fn key_lives_in_xdg_state_dir() {
        assert_eq!(
            default_key_path(env(&[("HOME", "/Users/alice")])).unwrap(),
            PathBuf::from("/Users/alice/.local/state/clipsy/secret.key")
        );
        assert_eq!(
            default_key_path(env(&[
                ("HOME", "/home/alice"),
                ("XDG_STATE_HOME", "/var/state")
            ]))
            .unwrap(),
            PathBuf::from("/var/state/clipsy/secret.key")
        );
    }

    #[test]
    fn key_path_needs_home_or_xdg_state_home() {
        assert!(default_key_path(env(&[])).is_err());
    }

    #[test]
    fn rejects_typo_in_client_permissions() {
        let text = format!("[clients.sandbox]\nid = \"{}\"\nreed = false\n", sandbox());

        let err = Config::parse(&text).unwrap_err();

        assert!(format!("{err:#}").contains("unknown field"), "{err:#}");
    }

    #[test]
    fn rejects_same_id_under_two_client_names() {
        let text = format!(
            "[clients.devbox]\nid = \"{id}\"\n[clients.old-devbox]\nid = \"{id}\"\nread = false\n",
            id = devbox()
        );

        let err = Config::parse(&text).unwrap_err();

        assert!(err.to_string().contains("same id"), "{err}");
    }

    #[test]
    fn rejects_malformed_relay_url() {
        assert!(Config::parse("relay_url = \"not a url\"").is_err());
    }

    #[test]
    fn rejects_relative_key_file() {
        let err = Config::parse("key_file = \"secret.key\"").unwrap_err();

        assert!(err.to_string().contains("absolute"), "{err}");
    }

    #[test]
    fn malformed_config_file_names_the_path() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[servers\nlaptop = ").unwrap();

        let err = Config::load(Some(&path)).unwrap_err();

        assert!(err.to_string().contains("config.toml"), "{err}");
    }

    #[test]
    fn empty_choice_falls_back_to_default_server() {
        let config = servers_only(
            Some("laptop"),
            &[("laptop", laptop()), ("desktop", desktop())],
        );

        assert_eq!(config.resolve_server(Some("")).unwrap(), laptop());
    }

    #[test]
    fn empty_or_relative_xdg_dirs_fall_back_to_home() {
        for bogus in ["", "relative/state"] {
            assert_eq!(
                default_key_path(env(&[("HOME", "/home/alice"), ("XDG_STATE_HOME", bogus)]))
                    .unwrap(),
                PathBuf::from("/home/alice/.local/state/clipsy/secret.key"),
                "XDG_STATE_HOME={bogus:?}"
            );
        }
    }

    #[test]
    fn empty_home_is_treated_as_unset() {
        assert!(default_key_path(env(&[("HOME", "")])).is_err());
    }

    #[test]
    fn discover_reads_user_config() {
        let dir = TempDir::new().unwrap();
        let config_dir = dir.path().join("clipsy");
        std::fs::create_dir(&config_dir).unwrap();
        std::fs::write(config_dir.join("config.toml"), nix_rendered_config()).unwrap();
        let xdg = dir.path().to_str().unwrap();

        let config =
            Config::discover(env(&[("HOME", "/nonexistent"), ("XDG_CONFIG_HOME", xdg)])).unwrap();

        assert_eq!(config.resolve_server(None).unwrap(), laptop());
    }

    #[test]
    fn unreadable_user_config_is_an_error_not_a_fallback() {
        let dir = TempDir::new().unwrap();
        let config_dir = dir.path().join("clipsy");
        std::fs::create_dir(&config_dir).unwrap();
        std::fs::write(config_dir.join("config.toml"), "").unwrap();
        std::fs::set_permissions(
            &config_dir,
            std::os::unix::fs::PermissionsExt::from_mode(0o000),
        )
        .unwrap();
        let xdg = dir.path().to_str().unwrap();

        let result = Config::discover(env(&[("XDG_CONFIG_HOME", xdg)]));

        std::fs::set_permissions(
            &config_dir,
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )
        .unwrap();
        assert!(result.is_err());
    }
}
