use anyhow::{bail, Result};
use clipsy::clipboard::{Clipboard, Entry, MemoryClipboard};
use clipsy::config::Config;
use clipsy::net::{close, send_request, Network, Timeouts};
use clipsy::protocol::{Request, Response, ALPN};
use clipsy::service::{ClipboardService, MAX_CONCURRENT_CONNECTIONS};
use iroh::endpoint::{ApplicationClose, Connection, ConnectionError, RecvStream, SendStream};
use iroh::protocol::Router;
use iroh::{Endpoint, EndpointAddr, EndpointId, SecretKey};
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn test_timeouts() -> Timeouts {
    Timeouts {
        connect: Duration::from_secs(2),
        exchange: Duration::from_millis(1500),
    }
}

fn patient_timeouts() -> Timeouts {
    Timeouts {
        exchange: Duration::from_secs(30),
        ..test_timeouts()
    }
}

struct Machine {
    endpoint: Endpoint,
    timeouts: Timeouts,
}

impl Machine {
    async fn new() -> Self {
        Self::with_timeouts(test_timeouts()).await
    }

    async fn patient() -> Self {
        Self::with_timeouts(patient_timeouts()).await
    }

    async fn with_timeouts(timeouts: Timeouts) -> Self {
        let endpoint = Network::LocalOnly
            .bind(SecretKey::generate(), vec![])
            .await
            .unwrap();
        Self { endpoint, timeouts }
    }

    fn id(&self) -> EndpointId {
        self.endpoint.id()
    }

    async fn send(&self, server: &Server, request: Request) -> Result<Response> {
        send_request(&self.endpoint, server.addr(), &request, &self.timeouts).await
    }

    async fn write(&self, server: &Server, content: &str) -> Result<Response> {
        let content = content.to_owned();
        self.send(
            server,
            Request::Write {
                content,
                concealed: false,
            },
        )
        .await
    }

    async fn read(&self, server: &Server) -> Result<Response> {
        self.send(server, Request::Read).await
    }

    async fn connect_raw(&self, server: &Server) -> Connection {
        self.endpoint.connect(server.addr(), ALPN).await.unwrap()
    }

    async fn stall(&self, server: &Server) -> (Connection, SendStream, RecvStream) {
        let conn = self.connect_raw(server).await;
        let (mut send, recv) = conn.open_bi().await.unwrap();
        send.write_all(b"{").await.unwrap();
        (conn, send, recv)
    }
}

struct Server {
    router: Router,
    clipboard: Arc<dyn Clipboard>,
    config: Arc<Mutex<String>>,
}

impl Server {
    async fn start(clipboard: impl Clipboard, config: String) -> Self {
        Self::start_with(clipboard, config, test_timeouts()).await
    }

    async fn start_with(clipboard: impl Clipboard, config: String, timeouts: Timeouts) -> Self {
        let endpoint = Network::LocalOnly
            .bind(SecretKey::generate(), vec![ALPN.to_vec()])
            .await
            .unwrap();
        let clipboard: Arc<dyn Clipboard> = Arc::new(clipboard);
        let config = Arc::new(Mutex::new(config));
        let shared = config.clone();
        let service = ClipboardService::new(
            clipboard.clone(),
            move || Config::parse(&shared.lock().unwrap()),
            timeouts,
        );
        let router = Router::builder(endpoint).accept(ALPN, service).spawn();
        Self {
            router,
            clipboard,
            config,
        }
    }

    async fn with_memory(initial: &str, config: String) -> Self {
        Self::start(MemoryClipboard::new(initial), config).await
    }

    fn addr(&self) -> EndpointAddr {
        let endpoint = self.router.endpoint();
        let port = endpoint
            .bound_sockets()
            .into_iter()
            .find(SocketAddr::is_ipv4)
            .unwrap()
            .port();
        EndpointAddr::new(endpoint.id()).with_ip_addr((Ipv4Addr::LOCALHOST, port).into())
    }

    fn clipboard(&self) -> String {
        self.clipboard.read().unwrap().text
    }

    fn set_config(&self, config: String) {
        *self.config.lock().unwrap() = config;
    }
}

fn client_entry(name: &str, id: EndpointId, read: bool, write: bool) -> String {
    format!("[clients.{name}]\nid = \"{id}\"\nread = {read}\nwrite = {write}\n")
}

fn allow(name: &str, client: &Machine) -> String {
    client_entry(name, client.id(), true, true)
}

#[tokio::test]
async fn allowed_client_writes_server_clipboard() {
    let devbox = Machine::new().await;
    let laptop = Server::with_memory("old", allow("devbox", &devbox)).await;

    let response = devbox
        .write(&laptop, "cargo test -- --nocapture\n")
        .await
        .unwrap();

    assert_eq!(response, Response::Written);
    assert_eq!(laptop.clipboard(), "cargo test -- --nocapture\n");
}

#[tokio::test]
async fn allowed_client_reads_server_clipboard() {
    let devbox = Machine::new().await;
    let laptop = Server::with_memory(
        "https://github.com/luizribeiro/clipsy",
        allow("devbox", &devbox),
    )
    .await;

    let response = devbox.read(&laptop).await.unwrap();

    assert_eq!(
        response,
        Response::Content {
            content: "https://github.com/luizribeiro/clipsy".into()
        }
    );
}

#[tokio::test]
async fn large_unicode_content_round_trips() {
    let devbox = Machine::patient().await;
    let laptop = Server::start_with(
        MemoryClipboard::default(),
        allow("devbox", &devbox),
        patient_timeouts(),
    )
    .await;
    let content = "héllo 📋 \"quoted\"\n".repeat(60_000);

    devbox.write(&laptop, &content).await.unwrap();
    let response = devbox.read(&laptop).await.unwrap();

    assert_eq!(response, Response::Content { content });
}

#[tokio::test]
async fn unknown_client_is_refused_with_its_id() {
    let devbox = Machine::new().await;
    let stranger = Machine::new().await;
    let laptop = Server::with_memory("password123", allow("devbox", &devbox)).await;

    let read = stranger.read(&laptop).await.unwrap_err().to_string();
    let write = stranger
        .write(&laptop, "pwned")
        .await
        .unwrap_err()
        .to_string();

    for err in [read, write] {
        assert!(err.contains("does not allow this machine"), "{err}");
        assert!(err.contains(&stranger.id().to_string()), "{err}");
    }
    assert_eq!(laptop.clipboard(), "password123");
}

async fn read_from_server_holding(entry: Entry, read_concealed: bool) -> Response {
    let devbox = Machine::new().await;
    let config = format!(
        "{}read_concealed = {read_concealed}\n",
        client_entry("devbox", devbox.id(), true, true)
    );
    let laptop = Server::start(MemoryClipboard::holding(entry), config).await;
    devbox.read(&laptop).await.unwrap()
}

#[tokio::test]
async fn concealed_entry_is_withheld_by_default() {
    let response = read_from_server_holding(Entry::concealed("hunter2"), false).await;

    assert_eq!(response, Response::Concealed);
}

#[tokio::test]
async fn concealed_entry_is_served_to_clients_allowed_to_read_it() {
    let response = read_from_server_holding(Entry::concealed("hunter2"), true).await;

    assert_eq!(
        response,
        Response::Content {
            content: "hunter2".into()
        }
    );
}

#[tokio::test]
async fn plain_entry_is_served_without_read_concealed() {
    let response = read_from_server_holding(Entry::plain("ls -la"), false).await;

    assert_eq!(
        response,
        Response::Content {
            content: "ls -la".into()
        }
    );
}

#[tokio::test]
async fn write_only_client_cannot_read() {
    let sandbox = Machine::new().await;
    let explicit = client_entry("sandbox", sandbox.id(), false, true);
    let by_default = format!("[clients.sandbox]\nid = \"{}\"\n", sandbox.id());

    for config in [explicit, by_default] {
        let laptop = Server::with_memory("password123", config).await;

        assert_eq!(sandbox.read(&laptop).await.unwrap(), Response::Denied);
        assert_eq!(
            sandbox.write(&laptop, "make build").await.unwrap(),
            Response::Written
        );
        assert_eq!(laptop.clipboard(), "make build");
    }
}

#[tokio::test]
async fn read_only_client_cannot_write() {
    let viewer = Machine::new().await;
    let config = client_entry("viewer", viewer.id(), true, false);
    let laptop = Server::with_memory("notes", config).await;

    assert_eq!(
        viewer.write(&laptop, "overwrite").await.unwrap(),
        Response::Denied
    );
    assert_eq!(laptop.clipboard(), "notes");
}

#[tokio::test]
async fn revoking_a_client_takes_effect_without_restart() {
    let devbox = Machine::new().await;
    let laptop = Server::with_memory("", allow("devbox", &devbox)).await;
    devbox.write(&laptop, "before").await.unwrap();

    laptop.set_config(String::new());

    let err = devbox.write(&laptop, "after").await.unwrap_err();

    assert!(err.to_string().contains("does not allow"), "{err}");
    assert_eq!(laptop.clipboard(), "before");
}

#[tokio::test]
async fn broken_config_refuses_everyone() {
    let devbox = Machine::new().await;
    let laptop = Server::with_memory("secret", allow("devbox", &devbox)).await;

    laptop.set_config("[clients.devbox\n".into());

    let err = devbox.read(&laptop).await.unwrap_err();

    assert!(err.to_string().contains("does not allow"), "{err}");
}

struct BrokenClipboard;

impl Clipboard for BrokenClipboard {
    fn read(&self) -> Result<Entry> {
        bail!("clipboard holds an image")
    }

    fn write(&self, _: Entry) -> Result<()> {
        bail!("pasteboard unavailable")
    }
}

#[tokio::test]
async fn clipboard_errors_are_reported_to_the_client() {
    let devbox = Machine::new().await;
    let laptop = Server::start(BrokenClipboard, allow("devbox", &devbox)).await;

    let read = devbox.read(&laptop).await.unwrap();
    let write = devbox.write(&laptop, "x").await.unwrap();

    assert_eq!(
        read,
        Response::Failed {
            reason: "clipboard holds an image".into()
        }
    );
    assert_eq!(
        write,
        Response::Failed {
            reason: "pasteboard unavailable".into()
        }
    );
}

#[tokio::test]
async fn several_clients_share_one_server() {
    let devbox = Machine::new().await;
    let desktop = Machine::new().await;
    let config = allow("devbox", &devbox) + &allow("desktop", &desktop);
    let laptop = Server::with_memory("", config).await;

    let (a, b) = tokio::join!(
        devbox.write(&laptop, "from devbox"),
        desktop.write(&laptop, "from desktop")
    );

    assert_eq!(
        (a.unwrap(), b.unwrap()),
        (Response::Written, Response::Written)
    );
    assert!(["from devbox", "from desktop"].contains(&laptop.clipboard().as_str()));
}

#[tokio::test]
async fn unreachable_server_is_an_error() {
    let devbox = Machine::new().await;
    let offline = EndpointAddr::new(SecretKey::generate().public());

    let err = send_request(&devbox.endpoint, offline, &Request::Read, &test_timeouts())
        .await
        .unwrap_err();

    assert!(err.to_string().contains("connecting to server"), "{err}");
}

fn close_code(conn: &Connection) -> Option<u64> {
    match conn.close_reason() {
        Some(ConnectionError::ApplicationClosed(ApplicationClose { error_code, .. })) => {
            Some(error_code.into_inner())
        }
        _ => None,
    }
}

async fn wait_for_server_close(conn: &Connection) -> Option<u64> {
    tokio::time::timeout(Duration::from_secs(5), conn.closed())
        .await
        .expect("server should close the connection");
    close_code(conn)
}

#[tokio::test]
async fn server_drops_client_that_never_finishes_its_request() {
    let devbox = Machine::new().await;
    let laptop = Server::with_memory("secret", allow("devbox", &devbox)).await;
    let (conn, _send, _recv) = devbox.stall(&laptop).await;

    assert_eq!(
        wait_for_server_close(&conn).await,
        Some(close::TIMED_OUT.into_inner())
    );
    assert_eq!(laptop.clipboard(), "secret");
}

#[tokio::test]
async fn server_drops_client_that_never_opens_a_stream() {
    let devbox = Machine::new().await;
    let laptop = Server::with_memory("", allow("devbox", &devbox)).await;

    let conn = devbox.connect_raw(&laptop).await;

    assert_eq!(
        wait_for_server_close(&conn).await,
        Some(close::TIMED_OUT.into_inner())
    );
}

#[tokio::test]
async fn server_refuses_connections_beyond_its_limit() {
    let devbox = Machine::new().await;
    let laptop = Server::start_with(
        MemoryClipboard::default(),
        allow("devbox", &devbox),
        patient_timeouts(),
    )
    .await;
    let mut stalled = Vec::new();
    for _ in 0..MAX_CONCURRENT_CONNECTIONS {
        stalled.push(devbox.stall(&laptop).await);
    }
    tokio::time::sleep(Duration::from_millis(100)).await;

    let err = devbox.read(&laptop).await.unwrap_err();

    assert!(err.to_string().contains("busy"), "{err}");
}

#[tokio::test]
async fn server_recovers_after_idle_connections_time_out() {
    let devbox = Machine::new().await;
    let laptop = Server::with_memory("still here", allow("devbox", &devbox)).await;
    let mut stalled = Vec::new();
    for _ in 0..MAX_CONCURRENT_CONNECTIONS {
        stalled.push(devbox.stall(&laptop).await);
    }
    for (conn, _, _) in &stalled {
        assert_eq!(
            wait_for_server_close(conn).await,
            Some(close::TIMED_OUT.into_inner())
        );
    }

    let response = devbox.read(&laptop).await.unwrap();

    assert_eq!(
        response,
        Response::Content {
            content: "still here".into()
        }
    );
}

#[tokio::test]
async fn garbage_request_gets_failed_response() {
    let devbox = Machine::new().await;
    let laptop = Server::with_memory("secret", allow("devbox", &devbox)).await;
    let conn = devbox.connect_raw(&laptop).await;

    let (mut send, mut recv) = conn.open_bi().await.unwrap();
    send.write_all(b"GET / HTTP/1.1\r\n\r\n").await.unwrap();
    send.finish().unwrap();
    let response: Response = clipsy::protocol::read_message(&mut recv).await.unwrap();

    assert!(matches!(response, Response::Failed { .. }), "{response:?}");
    assert_eq!(laptop.clipboard(), "secret");
}

#[tokio::test]
async fn oversized_request_is_rejected_without_touching_clipboard() {
    let devbox = Machine::new().await;
    let laptop = Server::start_with(
        MemoryClipboard::new("secret"),
        allow("devbox", &devbox),
        patient_timeouts(),
    )
    .await;
    let conn = devbox.connect_raw(&laptop).await;
    let oversized = format!(
        r#"{{"Write":{{"content":"{}"}}}}"#,
        "x".repeat(17 * 1024 * 1024)
    );

    let (mut send, mut recv) = conn.open_bi().await.unwrap();
    let _ = send.write_all(oversized.as_bytes()).await;
    let _ = send.finish();
    let response: Response = clipsy::protocol::read_message(&mut recv).await.unwrap();

    let Response::Failed { reason } = response else {
        panic!("expected failure, got {response:?}");
    };
    assert!(reason.contains("limit"), "{reason}");
    assert_eq!(laptop.clipboard(), "secret");
}

#[tokio::test]
async fn clipboard_too_large_to_send_is_reported() {
    let devbox = Machine::patient().await;
    let huge = "x".repeat(17 * 1024 * 1024);
    let laptop = Server::start_with(
        MemoryClipboard::new(huge),
        allow("devbox", &devbox),
        patient_timeouts(),
    )
    .await;

    let response = devbox.read(&laptop).await.unwrap();

    let Response::Failed { reason } = response else {
        panic!("expected failure");
    };
    assert!(reason.contains("limit"), "{reason}");
}

struct HungClipboard;

impl Clipboard for HungClipboard {
    fn read(&self) -> Result<Entry> {
        std::thread::sleep(Duration::from_secs(5));
        Ok("too late".into())
    }

    fn write(&self, _: Entry) -> Result<()> {
        std::thread::sleep(Duration::from_secs(5));
        Ok(())
    }
}

#[tokio::test]
async fn hung_clipboard_times_out_instead_of_hanging_the_client() {
    let devbox = Machine::new().await;
    let laptop = Server::start(HungClipboard, allow("devbox", &devbox)).await;

    let err = devbox.read(&laptop).await.unwrap_err();

    assert!(
        err.to_string().contains("gave up") || err.to_string().contains("did not reply"),
        "{err}"
    );
}

#[tokio::test]
async fn silent_server_hits_connect_timeout() {
    let devbox = Machine::new().await;
    let black_hole = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let silent = EndpointAddr::new(SecretKey::generate().public())
        .with_ip_addr(black_hole.local_addr().unwrap());
    let timeouts = Timeouts {
        connect: Duration::from_millis(300),
        ..test_timeouts()
    };

    let err = send_request(&devbox.endpoint, silent, &Request::Read, &timeouts)
        .await
        .unwrap_err();

    assert!(err.to_string().contains("is it online?"), "{err}");
}
