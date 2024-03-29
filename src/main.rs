mod client;
mod msg;
mod server;

use clap::{Args, Parser, Subcommand};
use server::start_server;
use tokio::io;

#[derive(Debug, Parser)] // requires `derive` feature
#[command(name = "clipsy")]
#[command(about = "A clipboard synchronizer", long_about = None)]
struct Cli {
    #[arg(short, long, global = true, default_value = "52697")]
    port: u16,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Args, Debug)]
struct ClientArgs {
    #[arg(short, long, default_value = "localhost")]
    server: String,
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
        #[command(flatten)]
        client_args: ClientArgs,
        /// The content to write to the server's clipboard
        content: Option<String>,
    },
    /// Reads content from the clipsy server
    Read {
        #[command(flatten)]
        client_args: ClientArgs,
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
        _ => client::handle_command(args).await?,
    }

    Ok(())
}
