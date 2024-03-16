use crate::msg::{read_message, send_message, Message};
use clipboard::{ClipboardContext, ClipboardProvider};
use tokio::io;
use tokio::net::TcpListener;

pub async fn start_server(port: u16) -> io::Result<()> {
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;

    println!("Server listening on port {}", port);

    loop {
        let (mut socket, _) = listener.accept().await?;

        match read_message(&mut socket).await {
            Ok(Message::ClipboardUpdate { content }) => {
                let mut ctx: ClipboardContext = ClipboardProvider::new().unwrap();
                ctx.set_contents(content).unwrap();
                println!("Clipboard updated!");

                // Send acknowledgment back
                send_message(&mut socket, Message::Acknowledgment)
                    .await
                    .unwrap();
            }
            Err(e) => println!("Error reading message: {}", e),
            _ => (),
        }
    }
}
