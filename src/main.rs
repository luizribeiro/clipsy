use clap::{App, Arg};
use clipboard::{ClipboardContext, ClipboardProvider};
use serde::{Deserialize, Serialize};
use serde_json::Result as JsonResult;
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[derive(Serialize, Deserialize)]
enum Message {
    ClipboardUpdate { content: String },
    Acknowledgment,
}

async fn send_message(socket: &mut TcpStream, message: Message) -> JsonResult<()> {
    let serialized = serde_json::to_string(&message)?;
    socket.write_all(serialized.as_bytes()).await.unwrap();
    Ok(())
}

async fn read_message(socket: &mut TcpStream) -> JsonResult<Message> {
    let mut buffer = vec![0; 1024]; // Buffer size might need adjustments
    let size = socket.read(&mut buffer).await.unwrap();
    serde_json::from_slice(&buffer[..size]).map_err(|e| e.into())
}

async fn start_server(port: u16) -> io::Result<()> {
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

async fn start_client(address: &str, data: String) -> io::Result<()> {
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

#[tokio::main]
async fn main() -> io::Result<()> {
    let matches = App::new("Clipboard Sync")
        .version("0.1.0")
        .author("Author Name")
        .about("Synchronizes clipboard between devices")
        .arg(
            Arg::with_name("server")
                .long("server")
                .help("Starts in server mode")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("client")
                .long("client")
                .value_name("ADDRESS")
                .help("Starts in client mode and connects to server at ADDRESS")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("port")
                .long("port")
                .value_name("PORT")
                .help("Port number for the server")
                .default_value("7878")
                .takes_value(true),
        )
        .get_matches();

    if matches.is_present("server") {
        let port = matches.value_of("port").unwrap().parse::<u16>().unwrap();
        start_server(port).await?;
    } else if let Some(address) = matches.value_of("client") {
        let mut ctx: ClipboardContext = ClipboardProvider::new().unwrap();
        let clipboard_contents = ctx.get_contents().unwrap();
        start_client(address, clipboard_contents).await?;
    } else {
        println!("No mode selected, use --server to start in server mode or --client [ADDRESS] to start in client mode.");
    }

    Ok(())
}
