mod client;
mod msg;
mod server;

use clap::{Parser, Subcommand};
use client::start_client;
use server::start_server;
use tokio::io::{self, AsyncReadExt};

#[derive(Debug, Parser)] // requires `derive` feature
#[command(name = "clipsy")]
#[command(about = "A clipboard synchronizer", long_about = None)]
struct Cli {
    #[arg(short, long, default_value = "7878")]
    port: u16,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Starts the clipsy server
    Serve {
        #[arg(short, long, default_value = "127.0.0.1")]
        bind: String,
    },
    /// Writes content to the clipsy server
    Write {
        #[arg(short, long, default_value = "localhost")]
        server: String,
        /// The content to write to the server's clipboard
        content: Option<String>,
    },
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let args = Cli::parse();
    match args.command {
        Commands::Serve { bind: cli_bind } => {
            let bind = format!("{}:{}", cli_bind, args.port);
            println!("Server listening on {}", bind);
            start_server(bind).await?;
        }
        Commands::Write {
            server,
            content: cli_content,
        } => {
            let address = format!("{}:{}", server, args.port);
            let mut content = String::new();
            if cli_content.is_none() {
                io::stdin().read_to_string(&mut content).await.unwrap();
            } else {
                content = cli_content.unwrap();
            }
            start_client(&address, content).await?;
        }
    }

    Ok(())
}
