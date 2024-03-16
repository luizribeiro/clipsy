use crate::msg::{read_message, send_message, Message};
use clipboard::{ClipboardContext, ClipboardProvider};
use tokio::io;
use tokio::net::TcpListener;

pub async fn start_server(addr: String) -> io::Result<()> {
    let listener = TcpListener::bind(addr).await?;

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
            Ok(Message::ClipboardRead) => {
                let mut ctx: ClipboardContext = ClipboardProvider::new().unwrap();
                let content = ctx.get_contents().unwrap();
                send_message(&mut socket, Message::ClipboardReadResponse { content })
                    .await
                    .unwrap();
            }
            Err(e) => println!("Error reading message: {}", e),
            _ => (),
        }
    }
}
