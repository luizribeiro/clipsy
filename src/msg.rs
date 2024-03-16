use serde::{Deserialize, Serialize};
use serde_json::Result as JsonResult;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Serialize, Deserialize)]
pub enum Message {
    ClipboardUpdate { content: String },
    ClipboardRead,
    ClipboardReadResponse { content: String },
    Acknowledgment,
}

pub async fn send_message(socket: &mut TcpStream, message: Message) -> JsonResult<()> {
    let serialized = serde_json::to_string(&message)?;
    socket.write_all(serialized.as_bytes()).await.unwrap();
    Ok(())
}

pub async fn read_message(socket: &mut TcpStream) -> JsonResult<Message> {
    let mut buffer = vec![0; 1024]; // Buffer size might need adjustments
    let size = socket.read(&mut buffer).await.unwrap();
    serde_json::from_slice(&buffer[..size]).map_err(|e| e.into())
}
