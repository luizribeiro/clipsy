mod client;
mod msg;
mod server;

use clap::{App, Arg};
use client::start_client;
use clipboard::{ClipboardContext, ClipboardProvider};
use server::start_server;
use tokio::io;

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
