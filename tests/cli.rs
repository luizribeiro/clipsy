use clipsy::clipboard::{Clipboard, MemoryClipboard};
use clipsy::config::Config;
use clipsy::net::{Network, Timeouts};
use clipsy::protocol::ALPN;
use clipsy::service::ClipboardService;
use iroh::protocol::Router;
use iroh::SecretKey;
use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn laptop_id() -> String {
    iroh::SecretKey::from_bytes(&[1; 32]).public().to_string()
}

fn laptop_config() -> String {
    format!("[servers]\nlaptop = \"{}\"\n", laptop_id())
}

struct Sandbox {
    home: TempDir,
}

impl Sandbox {
    fn new() -> Self {
        Self::with_config("")
    }

    fn with_config(text: &str) -> Self {
        let sandbox = Self {
            home: TempDir::new().unwrap(),
        };
        let dir = sandbox.home.path().join(".config/clipsy");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.toml"), text).unwrap();
        sandbox
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_clipsy"));
        command.args(args).env_clear().env("HOME", self.home.path());
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_with_env(args, &[])
    }

    fn run_with_env(&self, args: &[&str], vars: &[(&str, &str)]) -> Output {
        self.command(args)
            .envs(vars.iter().copied())
            .output()
            .unwrap()
    }

    fn run_with_stdin(&self, args: &[&str], stdin: &[u8]) -> Output {
        let mut child = self
            .command(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(stdin).unwrap();
        child.wait_with_output().unwrap()
    }

    fn id(&self) -> String {
        let output = self.run(&["id"]);
        assert!(output.status.success(), "{}", stderr(&output));
        stdout(&output).trim().to_owned()
    }

    fn key_path(&self) -> std::path::PathBuf {
        self.home.path().join(".local/state/clipsy/secret.key")
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn is_node_id(text: &str) -> bool {
    text.len() == 64 && text.chars().all(|c| c.is_ascii_hexdigit())
}

#[test]
fn id_creates_key_and_prints_stable_node_id() {
    let sandbox = Sandbox::new();

    let first = sandbox.run(&["id"]);
    let second = sandbox.run(&["id"]);

    assert!(first.status.success(), "{}", stderr(&first));
    assert!(is_node_id(stdout(&first).trim()), "{}", stdout(&first));
    assert_eq!(stdout(&first), stdout(&second));
    assert!(sandbox.key_path().exists());
}

#[test]
fn id_uses_key_file_from_config() {
    let keys = TempDir::new().unwrap();
    let key_file = keys.path().join("clipsy.key");
    let sandbox = Sandbox::with_config(&format!("key_file = \"{}\"\n", key_file.display()));

    let output = sandbox.run(&["id"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(key_file.exists());
    assert!(!sandbox.key_path().exists());
}

#[test]
fn write_without_configured_server_explains_how_to_pick_one() {
    let sandbox = Sandbox::new();

    let output = sandbox.run(&["write", "hello"]);

    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("no server configured"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn read_with_unknown_server_name_lists_known_servers() {
    let sandbox = Sandbox::with_config(&laptop_config());

    let output = sandbox.run(&["read", "--server", "macbook"]);

    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("known servers: laptop"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn clipsy_server_env_selects_server() {
    let sandbox = Sandbox::with_config(&laptop_config());

    let output = sandbox.run_with_env(&["read"], &[("CLIPSY_SERVER", "desktop")]);

    assert!(
        stderr(&output).contains("unknown server `desktop`"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn broken_config_is_reported_with_its_path() {
    let sandbox = Sandbox::with_config("[servers\n");

    let output = sandbox.run(&["id"]);

    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("config.toml"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn explicit_config_flag_must_exist() {
    let sandbox = Sandbox::new();
    let missing = Path::new("/nonexistent/clipsy.toml");

    let output = sandbox.run(&["--config", missing.to_str().unwrap(), "id"]);

    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("/nonexistent/clipsy.toml"),
        "{}",
        stderr(&output)
    );
}

struct LoopbackServer {
    router: Router,
    clipboard: Arc<MemoryClipboard>,
}

impl LoopbackServer {
    async fn start(initial: &str, clients: &str) -> Self {
        let clipboard = Arc::new(MemoryClipboard::new(initial));
        let endpoint = Network::LocalOnly
            .bind(SecretKey::generate(), vec![ALPN.to_vec()])
            .await
            .unwrap();
        let config = clients.to_owned();
        let service = ClipboardService::new(
            clipboard.clone(),
            move || Config::parse(&config),
            Timeouts::default(),
        );
        let router = Router::builder(endpoint).accept(ALPN, service).spawn();
        Self { router, clipboard }
    }

    fn target(&self) -> String {
        let endpoint = self.router.endpoint();
        let port = endpoint
            .bound_sockets()
            .into_iter()
            .find(SocketAddr::is_ipv4)
            .unwrap()
            .port();
        format!("{}@{}:{port}", endpoint.id(), Ipv4Addr::LOCALHOST)
    }

    fn clipboard(&self) -> String {
        self.clipboard.read().unwrap()
    }
}

fn client(name: &str, id: &str, read: bool, write: bool) -> String {
    format!("[clients.{name}]\nid = \"{id}\"\nread = {read}\nwrite = {write}\n")
}

#[tokio::test(flavor = "multi_thread")]
async fn write_argument_then_read_round_trips_through_the_binary() {
    let devbox = Sandbox::new();
    let laptop = LoopbackServer::start("old", &client("devbox", &devbox.id(), true, true)).await;

    let written = devbox.run(&[
        "write",
        "--server",
        &laptop.target(),
        "git push --force-with-lease",
    ]);
    let read = devbox.run(&["read", "--server", &laptop.target()]);

    assert!(written.status.success(), "{}", stderr(&written));
    assert_eq!(stdout(&written), "");
    assert_eq!(laptop.clipboard(), "git push --force-with-lease");
    assert!(read.status.success(), "{}", stderr(&read));
    assert_eq!(stdout(&read), "git push --force-with-lease");
}

#[tokio::test(flavor = "multi_thread")]
async fn write_without_argument_reads_stdin_like_tmux_copy_command() {
    let devbox = Sandbox::new();
    let laptop = LoopbackServer::start("", &client("devbox", &devbox.id(), true, true)).await;
    let selection = "fn main() {\n    println!(\"héllo\");\n}\n";

    let output = devbox.run_with_stdin(
        &["write", "--server", &laptop.target()],
        selection.as_bytes(),
    );

    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(laptop.clipboard(), selection);
}

#[tokio::test(flavor = "multi_thread")]
async fn empty_stdin_clears_the_clipboard() {
    let devbox = Sandbox::new();
    let laptop = LoopbackServer::start("secret", &client("devbox", &devbox.id(), true, true)).await;

    let output = devbox.run_with_stdin(&["write", "--server", &laptop.target()], b"");

    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(laptop.clipboard(), "");
}

#[tokio::test(flavor = "multi_thread")]
async fn read_prints_large_clipboard_exactly() {
    let devbox = Sandbox::new();
    let content = "line with trailing spaces   \n".repeat(40_000);
    let laptop = LoopbackServer::start(&content, &client("devbox", &devbox.id(), true, true)).await;

    let output = devbox.run(&["read", "--server", &laptop.target()]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        output.stdout == content.as_bytes(),
        "read {} bytes",
        output.stdout.len()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn clipsy_server_env_carries_the_server_like_ssh_setenv() {
    let devbox = Sandbox::new();
    let desktop =
        LoopbackServer::start("from desktop", &client("devbox", &devbox.id(), true, true)).await;

    let output = devbox.run_with_env(&["read"], &[("CLIPSY_SERVER", &desktop.target())]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "from desktop");
}

#[tokio::test(flavor = "multi_thread")]
async fn write_only_client_is_told_it_cannot_read() {
    let sandbox = Sandbox::new();
    let laptop = LoopbackServer::start(
        "password123",
        &client("sandbox", &sandbox.id(), false, true),
    )
    .await;

    let output = sandbox.run(&["read", "--server", &laptop.target()]);

    assert!(!output.status.success());
    assert_eq!(stdout(&output), "");
    assert!(
        stderr(&output).contains("does not allow this machine to read"),
        "{}",
        stderr(&output)
    );
    assert!(
        stderr(&output).contains("`read = true`"),
        "{}",
        stderr(&output)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn unpaired_machine_is_told_its_id() {
    let stranger = Sandbox::new();
    let laptop = LoopbackServer::start("password123", "").await;

    let output = stranger.run(&["read", "--server", &laptop.target()]);

    assert!(!output.status.success());
    assert!(
        stderr(&output).contains(&stranger.id()),
        "{}",
        stderr(&output)
    );
    assert_eq!(laptop.clipboard(), "password123");
}

#[test]
fn bad_server_fails_before_waiting_on_stdin() {
    let sandbox = Sandbox::new();
    let mut child = sandbox
        .command(&["write", "--server", "macbook"])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _open_stdin = child.stdin.take();
    let started = Instant::now();

    while child.try_wait().unwrap().is_none() {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "write waited for stdin"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    assert!(!child.wait().unwrap().success());
}

#[test]
fn non_utf8_stdin_is_explained() {
    let sandbox = Sandbox::with_config(&laptop_config());

    let output = sandbox.run_with_stdin(&["write"], &[0xff, 0xfe, 0x00]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("UTF-8"), "{}", stderr(&output));
}

#[test]
fn empty_clipsy_server_falls_back_to_config() {
    let sandbox = Sandbox::new();

    let output = sandbox.run_with_env(&["read"], &[("CLIPSY_SERVER", "")]);

    assert!(
        stderr(&output).contains("no server configured"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn failed_request_leaves_no_key_behind() {
    let sandbox = Sandbox::new();

    sandbox.run(&["read", "--server", "macbook"]);

    assert!(!sandbox.key_path().exists());
}

#[test]
fn malformed_relay_url_is_rejected() {
    let output = Sandbox::new().run_with_env(&["id"], &[("CLIPSY_RELAY_URL", "not a url")]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("relay-url"), "{}", stderr(&output));
}

#[test]
fn serve_announces_its_id_and_stops_cleanly_on_sigterm() {
    let laptop = Sandbox::new();
    let expected = format!("serving clipboard as {}", laptop.id());
    let mut child = laptop
        .command(&["serve", "--relay-url", "https://127.0.0.1:1"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    let mut stdout = std::io::BufReader::new(child.stdout.take().unwrap());
    let mut announcement = String::new();
    let mut listening = String::new();
    std::io::BufRead::read_line(&mut stdout, &mut announcement).unwrap();
    std::io::BufRead::read_line(&mut stdout, &mut listening).unwrap();
    Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "serve ignored SIGTERM"
        );
        std::thread::sleep(Duration::from_millis(20));
    };

    assert_eq!(announcement.trim(), expected);
    assert!(
        listening.starts_with("listening on UDP port "),
        "{listening}"
    );
    assert!(status.success(), "{status}");
}

#[test]
fn version_flag_prints_crate_version() {
    let output = Sandbox::new().run(&["--version"]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("clipsy {}\n", env!("CARGO_PKG_VERSION"))
    );
}
