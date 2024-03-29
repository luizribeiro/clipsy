use crate::msg::{read_message, send_message, Message};
use crate::{Cli, ClientArgs, Commands};
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub async fn handle_command(args: Cli) -> io::Result<()> {
    match args.command {
        Commands::Write {
            client_args: ClientArgs { server },
            content: cli_content,
        } => {
            let mut data = String::new();
            if cli_content.is_none() {
                io::stdin().read_to_string(&mut data).await?;
            } else {
                data = cli_content.unwrap();
            }

            let mut stream = connect_to_server(&server, args.port).await?;
            send_message(&mut stream, Message::ClipboardUpdate { content: data }).await?;
            match read_message(&mut stream).await {
                Ok(Message::Acknowledgment) => {
                    println!("Clipboard update acknowledged by server.");
                }
                Err(e) => println!("Error receiving acknowledgment: {}", e),
                _ => panic!("Invalid message received"),
            }
        }
        Commands::Read {
            client_args: ClientArgs { server },
        } => {
            let mut stream = connect_to_server(&server, args.port).await?;
            send_message(&mut stream, Message::ClipboardRead).await?;
            match read_message(&mut stream).await {
                Ok(Message::ClipboardReadResponse { content }) => {
                    io::stdout().write_all(content.as_bytes()).await?;
                }
                Err(e) => println!("Error reading clipboard content: {}", e),
                _ => panic!("Invalid message received"),
            }
        }
        _ => panic!("Invalid command"),
    }

    Ok(())
}

async fn connect_to_server(server: &str, port: u16) -> io::Result<TcpStream> {
    let address = format!("{}:{}", server, port);
    let stream = TcpStream::connect(address).await?;
    Ok(stream)
}
