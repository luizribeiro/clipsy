use clipsy::clipboard::{Clipboard, MemoryClipboard};
use clipsy::config::Config;
use clipsy::net::{send_request, Network, Timeouts};
use clipsy::protocol::{Request, Response, ALPN};
use clipsy::service::ClipboardService;
use iroh::endpoint::presets;
use iroh::protocol::Router;
use iroh::{Endpoint, RelayMode, SecretKey};
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

const LIVE_TIMEOUT: Duration = Duration::from_secs(30);

async fn bind_relay_only(network: &Network, secret: SecretKey, alpns: Vec<Vec<u8>>) -> Endpoint {
    let builder = match network {
        Network::Relay(url) => {
            Endpoint::builder(presets::Minimal).relay_mode(RelayMode::custom([url.clone()]))
        }
        Network::N0 => Endpoint::builder(presets::N0),
        Network::LocalOnly => panic!("a relay-only endpoint needs a relay"),
    };
    builder
        .clear_ip_transports()
        .secret_key(secret)
        .alpns(alpns)
        .bind()
        .await
        .expect("binding a relay-only endpoint")
}

async fn round_trip_through_relay_only(network: Network) {
    let client_key = SecretKey::generate();
    let config = format!("[clients.probe]\nid = \"{}\"\n", client_key.public());
    let clipboard = Arc::new(MemoryClipboard::default());
    let server = bind_relay_only(&network, SecretKey::generate(), vec![ALPN.to_vec()]).await;
    server.online().await;
    let server_id = server.id();
    let service = ClipboardService::new(
        clipboard.clone(),
        move || Config::parse(&config),
        Timeouts::default(),
    );
    let router = Router::builder(server).accept(ALPN, service).spawn();
    let client = bind_relay_only(&network, client_key, vec![]).await;

    let started = Instant::now();
    let request = Request::Write {
        content: "hello over iroh".into(),
    };
    let addr = network.server_addr(server_id.into());
    let response = send_request(&client, addr, &request, &Timeouts::default())
        .await
        .unwrap_or_else(|e| panic!("write via {network:?}: {e:#}"));
    println!(
        "{network:?}: connect and write over relay took {:?}",
        started.elapsed()
    );

    assert_eq!(response, Response::Written);
    assert_eq!(clipboard.read().unwrap(), "hello over iroh");
    client.close().await;
    router.shutdown().await.unwrap();
}

async fn within_live_timeout(test: impl Future<Output = ()>) {
    tokio::time::timeout(LIVE_TIMEOUT, test)
        .await
        .expect("relay did not complete the round trip in time");
}

#[tokio::test]
#[ignore = "needs internet access and the relay in CLIPSY_RELAY_URL"]
async fn round_trips_through_relay_from_clipsy_relay_url() {
    let relay = std::env::var("CLIPSY_RELAY_URL")
        .expect("set CLIPSY_RELAY_URL")
        .parse()
        .expect("CLIPSY_RELAY_URL must be a relay URL");
    within_live_timeout(round_trip_through_relay_only(Network::Relay(relay))).await;
}

#[tokio::test]
#[ignore = "needs internet access"]
async fn round_trips_through_n0_dns_and_relays() {
    within_live_timeout(round_trip_through_relay_only(Network::N0)).await;
}
