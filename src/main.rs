use anyhow::{bail, Context as _, Result};
use clap::{Args, Parser, Subcommand};
use clipsy::clipboard::SystemClipboard;
use clipsy::config::{self, Config, ServerTarget};
use clipsy::keys;
use clipsy::net::{send_request, Network, Timeouts};
use clipsy::protocol::{Request, Response, ALPN};
use clipsy::service::ClipboardService;
use iroh::protocol::Router;
use iroh::{RelayUrl, SecretKey};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};

#[derive(Debug, Parser)]
#[command(name = "clipsy", about = "A clipboard synchronizer", long_about = None)]
struct Cli {
    /// Config file [default: ~/.config/clipsy/config.toml, then /etc/clipsy/config.toml]
    #[arg(long, global = true, env = "CLIPSY_CONFIG")]
    config: Option<PathBuf>,
    /// Relay server URL, overriding relay_url from the config file
    #[arg(long, global = true, env = "CLIPSY_RELAY_URL")]
    relay_url: Option<RelayUrl>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Serves this machine's clipboard to the clients allowed in the config
    Serve,
    /// Writes content to a server's clipboard
    Write {
        #[command(flatten)]
        target: Target,
        /// The content to write; read from stdin when omitted
        content: Option<String>,
    },
    /// Prints a server's clipboard
    Read {
        #[command(flatten)]
        target: Target,
    },
    /// Prints this machine's node id, creating its key on first run
    Id,
}

#[derive(Debug, Args)]
struct Target {
    /// Server name from the config, a node id, or NODE_ID@IP:PORT [default: default_server]
    #[arg(short, long, env = "CLIPSY_SERVER")]
    server: Option<String>,
}

struct Context {
    config: Config,
    config_path: Option<PathBuf>,
    network: Network,
}

impl Context {
    fn load(cli: &Cli) -> Result<Self> {
        let config = load_config(cli.config.as_deref())?;
        Ok(Self {
            network: Network::from_relay(cli.relay_url.clone().or(config.relay_url.clone())),
            config_path: cli.config.clone(),
            config,
        })
    }

    fn secret(&self) -> Result<SecretKey> {
        let path = match &self.config.key_file {
            Some(path) => path.clone(),
            None => config::default_key_path(config::process_env)?,
        };
        keys::load_or_create(&path)
    }

    fn server(&self, target: &Target) -> Result<ServerTarget> {
        self.config.resolve_server(target.server.as_deref())
    }
}

fn load_config(path: Option<&Path>) -> Result<Config> {
    match path {
        Some(path) => Config::load(Some(path)),
        None => Config::discover(config::process_env),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let context = Context::load(&cli)?;
    match cli.command {
        Command::Serve => serve(context).await,
        Command::Write { target, content } => {
            let server = context.server(&target)?;
            let content = match content {
                Some(content) => content,
                None => read_stdin()?,
            };
            request(&context, server, Request::Write { content }).await
        }
        Command::Read { target } => {
            let server = context.server(&target)?;
            request(&context, server, Request::Read).await
        }
        Command::Id => {
            println!("{}", context.secret()?.public());
            Ok(())
        }
    }
}

async fn serve(context: Context) -> Result<()> {
    let endpoint = context
        .network
        .bind(context.secret()?, vec![ALPN.to_vec()])
        .await?;
    println!("serving clipboard as {}", endpoint.id());
    let config_path = context.config_path;
    let service = ClipboardService::new(
        Arc::new(SystemClipboard),
        move || load_config(config_path.as_deref()),
        Timeouts::default(),
    );
    let router = Router::builder(endpoint).accept(ALPN, service).spawn();
    shutdown_requested().await?;
    router.shutdown().await?;
    Ok(())
}

async fn shutdown_requested() -> Result<()> {
    let mut terminate = signal(SignalKind::terminate())?;
    tokio::select! {
        interrupted = tokio::signal::ctrl_c() => interrupted?,
        _ = terminate.recv() => {}
    }
    Ok(())
}

async fn request(context: &Context, server: ServerTarget, request: Request) -> Result<()> {
    let endpoint = context.network.bind(context.secret()?, vec![]).await?;
    let addr = context.network.server_addr(server);
    let response = send_request(&endpoint, addr, &request, &Timeouts::default()).await;
    endpoint.close().await;
    let id = server.id;
    match response? {
        Response::Written => Ok(()),
        Response::Content { content } => {
            let mut stdout = std::io::stdout().lock();
            stdout.write_all(content.as_bytes())?;
            stdout.flush()?;
            Ok(())
        }
        Response::Denied => bail!(
            "server {id} does not allow this machine to {}",
            action(&request)
        ),
        Response::Failed { reason } => {
            bail!("server {id} could not {}: {reason}", action(&request))
        }
    }
}

fn action(request: &Request) -> &'static str {
    match request {
        Request::Write { .. } => "write its clipboard",
        Request::Read => "read its clipboard",
    }
}

fn read_stdin() -> Result<String> {
    let mut content = String::new();
    std::io::stdin()
        .read_to_string(&mut content)
        .context("clipboard content must be UTF-8 text")?;
    Ok(content)
}
