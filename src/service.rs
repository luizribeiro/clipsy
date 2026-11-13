use crate::clipboard::{Clipboard, Entry};
use crate::config::{Client, Config};
use crate::net::{close, Timeouts};
use crate::protocol::{read_message, write_message, Request, Response};
use anyhow::{anyhow, Result};
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler};
use iroh::EndpointId;
use std::collections::HashMap;
use std::fmt::{self, Display};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use tokio::time::timeout;

pub const MAX_CONCURRENT_CONNECTIONS: usize = 8;
const REFUSAL_LOG_INTERVAL: Duration = Duration::from_secs(60);
const REFUSAL_LOG_NODES: usize = 16;

type ConfigLoader = dyn Fn() -> Result<Config> + Send + Sync;

#[derive(Debug, PartialEq, Eq)]
pub enum Access {
    Allowed { name: String, client: Client },
    Unknown,
    Unconfigured(String),
}

pub fn access(config: Result<Config>, remote: &EndpointId) -> Access {
    match config {
        Ok(config) => match config.client(remote) {
            Some((name, client)) => Access::Allowed {
                name: name.to_owned(),
                client: *client,
            },
            None => Access::Unknown,
        },
        Err(e) => Access::Unconfigured(format!("{e:#}")),
    }
}

#[derive(Default)]
struct RefusalLog(HashMap<EndpointId, Instant>);

impl RefusalLog {
    fn should_log(&mut self, remote: EndpointId, now: Instant) -> bool {
        self.0
            .retain(|_, at| now.duration_since(*at) < REFUSAL_LOG_INTERVAL);
        if self.0.contains_key(&remote) || self.0.len() >= REFUSAL_LOG_NODES {
            return false;
        }
        self.0.insert(remote, now);
        true
    }
}

#[derive(Clone)]
pub struct ClipboardService {
    clipboard: Arc<dyn Clipboard>,
    load_config: Arc<ConfigLoader>,
    timeouts: Timeouts,
    connections: Arc<Semaphore>,
    clipboard_access: Arc<Semaphore>,
    refusal_log: Arc<Mutex<RefusalLog>>,
}

impl fmt::Debug for ClipboardService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ClipboardService")
    }
}

impl ClipboardService {
    pub fn new(
        clipboard: Arc<dyn Clipboard>,
        load_config: impl Fn() -> Result<Config> + Send + Sync + 'static,
        timeouts: Timeouts,
    ) -> Self {
        Self {
            clipboard,
            load_config: Arc::new(load_config),
            timeouts,
            connections: Arc::new(Semaphore::new(MAX_CONCURRENT_CONNECTIONS)),
            clipboard_access: Arc::new(Semaphore::new(1)),
            refusal_log: Arc::default(),
        }
    }

    async fn access(&self, remote: &EndpointId) -> Access {
        let load_config = self.load_config.clone();
        let config = tokio::task::spawn_blocking(move || (*load_config)())
            .await
            .unwrap_or_else(|e| Err(anyhow!("config loader failed: {e}")));
        access(config, remote)
    }

    fn refuse(&self, conn: &Connection, why: &str) -> Result<(), AcceptError> {
        conn.close(close::NOT_ALLOWED, b"not allowed");
        let remote = conn.remote_id();
        let mut log = self.refusal_log.lock().unwrap_or_else(|e| e.into_inner());
        if log.should_log(remote, Instant::now()) {
            println!(
                "refused {remote}: {why} (further refusals of this node muted for {REFUSAL_LOG_INTERVAL:?})"
            );
        }
        Ok(())
    }

    async fn serve(&self, conn: &Connection, name: &str, client: Client) -> Result<()> {
        let (mut send, mut recv) = conn.accept_bi().await?;
        let request = read_message::<Request>(&mut recv).await;
        let _ = recv.stop(close::DONE);
        let response = match request {
            Ok(request) => self.respond(name, client, request).await,
            Err(e) => failed(format!("{e:#}")),
        };
        if let Err(e) = write_message(&mut send, &response).await {
            write_message(&mut send, &failed(format!("{e:#}"))).await?;
        }
        send.finish()?;
        conn.closed().await;
        Ok(())
    }

    async fn respond(&self, name: &str, client: Client, request: Request) -> Response {
        if !client.allows(&request) {
            println!("denied {} from {name}", describe(&request));
            return Response::Denied;
        }
        println!("{} from {name}", describe(&request));
        let Ok(turn) = self.clipboard_access.clone().acquire_owned().await else {
            return failed("server is shutting down");
        };
        let clipboard = self.clipboard.clone();
        let name = name.to_owned();
        let outcome = tokio::task::spawn_blocking(move || {
            let _turn = turn;
            match request {
                Request::Write { content, concealed } => clipboard
                    .write(Entry {
                        text: content,
                        concealed,
                    })
                    .map(|()| Response::Written),
                Request::Read => clipboard.read().map(|entry| {
                    if entry.concealed && !client.read_concealed {
                        println!("withheld a concealed entry from {name}; it lacks read_concealed");
                        Response::Concealed
                    } else {
                        Response::Content {
                            content: entry.text,
                        }
                    }
                }),
            }
        })
        .await;
        match outcome {
            Ok(Ok(response)) => response,
            Ok(Err(e)) => failed(format!("{e:#}")),
            Err(e) => failed(format!("clipboard task failed: {e}")),
        }
    }
}

fn failed(reason: impl Display) -> Response {
    Response::Failed {
        reason: reason.to_string(),
    }
}

fn describe(request: &Request) -> String {
    match request {
        Request::Write {
            content,
            concealed: false,
        } => format!("write of {} bytes", content.len()),
        Request::Write {
            content,
            concealed: true,
        } => format!("concealed write of {} bytes", content.len()),
        Request::Read => "read".into(),
    }
}

impl ProtocolHandler for ClipboardService {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let Ok(_slot) = self.connections.clone().try_acquire_owned() else {
            conn.close(close::BUSY, b"busy");
            return Ok(());
        };
        let remote = conn.remote_id();
        let (name, client) = match self.access(&remote).await {
            Access::Allowed { name, client } => (name, client),
            Access::Unknown => return self.refuse(&conn, "unknown node"),
            Access::Unconfigured(e) => {
                return self.refuse(&conn, &format!("config could not be loaded: {e}"))
            }
        };
        match timeout(self.timeouts.exchange, self.serve(&conn, &name, client)).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                println!("request from {name} failed: {e:#}");
                conn.close(close::INTERNAL, b"internal error");
            }
            Err(_) => {
                println!("request from {name} timed out");
                conn.close(close::TIMED_OUT, b"timed out");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;

    fn id(seed: u8) -> EndpointId {
        SecretKey::from_bytes(&[seed; 32]).public()
    }

    fn config() -> Result<Config> {
        Config::parse(&format!(
            "[clients.devbox]\nid = \"{}\"\n[clients.sandbox]\nid = \"{}\"\nread = false\n",
            id(3),
            id(4)
        ))
    }

    #[test]
    fn refusals_are_logged_once_per_node_per_interval() {
        let mut log = RefusalLog::default();
        let start = Instant::now();

        assert!(log.should_log(id(7), start));
        assert!(!log.should_log(id(7), start + Duration::from_secs(5)));
        assert!(log.should_log(id(8), start + Duration::from_secs(5)));
        assert!(log.should_log(id(7), start + REFUSAL_LOG_INTERVAL));
    }

    #[test]
    fn refusal_log_stops_logging_new_nodes_at_its_cap() {
        let mut log = RefusalLog::default();
        let start = Instant::now();
        let logged = (0..=u8::MAX)
            .filter(|seed| log.should_log(id(*seed), start))
            .count();

        assert_eq!(logged, REFUSAL_LOG_NODES);
        assert!(log.should_log(id(0), start + REFUSAL_LOG_INTERVAL));
    }

    #[test]
    fn refusal_log_forgets_nodes_after_the_interval() {
        let mut log = RefusalLog::default();
        let start = Instant::now();
        for seed in 10..20 {
            log.should_log(id(seed), start);
        }

        log.should_log(id(7), start + REFUSAL_LOG_INTERVAL);

        assert_eq!(log.0.len(), 1);
    }

    #[test]
    fn listed_client_is_allowed_with_its_permissions() {
        let Access::Allowed { name, client } = access(config(), &id(4)) else {
            panic!("sandbox should be allowed");
        };

        assert_eq!(name, "sandbox");
        assert!(!client.read && client.write);
    }

    #[test]
    fn unlisted_node_is_unknown() {
        assert_eq!(access(config(), &id(9)), Access::Unknown);
    }

    #[test]
    fn config_error_refuses_even_listed_clients() {
        let broken = Err(anyhow!("parsing /etc/clipsy/config.toml"));

        let Access::Unconfigured(reason) = access(broken, &id(3)) else {
            panic!("broken config must not allow anyone");
        };

        assert!(reason.contains("/etc/clipsy/config.toml"));
    }
}
