use crate::msg::{read_message, send_message, Message};
use tokio::io;
use tokio::net::TcpStream;

pub async fn start_client(address: &str, data: String) -> io::Result<()> {
    let mut stream = TcpStream::connect(address).await?;

    send_message(&mut stream, Message::ClipboardUpdate { content: data })
        .await
        .unwrap();

    match read_message(&mut stream).await {
        Ok(Message::Acknowledgment) => {
            println!("Clipboard update acknowledged by server.");
        }
        Err(e) => println!("Error receiving acknowledgment: {}", e),
        _ => (),
    }

    Ok(())
}
