use crate::protocol::{read_message, write_message, Request, Response, ALPN};
use anyhow::{anyhow, Context, Result};
use iroh::endpoint::{presets, ApplicationClose, Connection, ConnectionError};
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, RelayUrl, SecretKey};
use std::time::Duration;
use tokio::time::timeout;

pub mod close {
    use iroh::endpoint::VarInt;

    pub const DONE: VarInt = VarInt::from_u32(0);
    pub const NOT_ALLOWED: VarInt = VarInt::from_u32(403);
    pub const TIMED_OUT: VarInt = VarInt::from_u32(408);
    pub const INTERNAL: VarInt = VarInt::from_u32(500);
    pub const BUSY: VarInt = VarInt::from_u32(503);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeouts {
    pub connect: Duration,
    pub exchange: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(5),
            exchange: Duration::from_secs(30),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Network {
    N0,
    Relay(RelayUrl),
    LocalOnly,
}

impl Network {
    pub fn from_relay(relay: Option<RelayUrl>) -> Self {
        relay.map_or(Self::N0, Self::Relay)
    }

    pub async fn bind(&self, secret: SecretKey, alpns: Vec<Vec<u8>>) -> Result<Endpoint> {
        let builder = match self {
            Self::N0 => Endpoint::builder(presets::N0),
            Self::Relay(url) => {
                Endpoint::builder(presets::Minimal).relay_mode(RelayMode::custom([url.clone()]))
            }
            Self::LocalOnly => Endpoint::builder(presets::Minimal).relay_mode(RelayMode::Disabled),
        };
        let endpoint = builder.secret_key(secret).alpns(alpns).bind().await?;
        Ok(endpoint)
    }

    pub fn server_addr(&self, id: EndpointId) -> EndpointAddr {
        match self {
            Self::Relay(url) => EndpointAddr::new(id).with_relay_url(url.clone()),
            Self::N0 | Self::LocalOnly => EndpointAddr::new(id),
        }
    }
}

pub async fn send_request(
    endpoint: &Endpoint,
    server: EndpointAddr,
    request: &Request,
    timeouts: &Timeouts,
) -> Result<Response> {
    let server_id = server.id;
    let conn = timeout(timeouts.connect, endpoint.connect(server, ALPN))
        .await
        .map_err(|_| {
            anyhow!(
                "server {server_id} did not answer within {:?}; is it online?",
                timeouts.connect
            )
        })?
        .with_context(|| format!("connecting to server {server_id}"))?;
    let exchanged = match timeout(timeouts.exchange, exchange(&conn, request)).await {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(e)) => Err(closed_by_server(&conn, endpoint.id()).unwrap_or(e)),
        Err(_) => Err(closed_by_server(&conn, endpoint.id()).unwrap_or_else(|| {
            anyhow!(
                "server {server_id} did not reply within {:?}",
                timeouts.exchange
            )
        })),
    };
    conn.close(close::DONE, b"done");
    exchanged
}

async fn exchange(conn: &Connection, request: &Request) -> Result<Response> {
    let (mut send, mut recv) = conn.open_bi().await?;
    write_message(&mut send, request).await?;
    send.finish()?;
    read_message(&mut recv).await
}

fn closed_by_server(conn: &Connection, own_id: EndpointId) -> Option<anyhow::Error> {
    let Some(ConnectionError::ApplicationClosed(ApplicationClose { error_code, .. })) =
        conn.close_reason()
    else {
        return None;
    };
    let server = conn.remote_id();
    Some(match error_code {
        close::NOT_ALLOWED => anyhow!(
            "server {server} does not allow this machine; add its id ({own_id}) to the server's clients"
        ),
        close::BUSY => anyhow!("server {server} is busy; try again"),
        close::TIMED_OUT => anyhow!("server {server} gave up waiting for the request"),
        close::INTERNAL => anyhow!("server {server} hit an internal error; check its log"),
        code => anyhow!("server {server} closed the connection (code {code})"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relay() -> RelayUrl {
        "https://relay.example.com".parse().unwrap()
    }

    fn laptop() -> EndpointId {
        SecretKey::from_bytes(&[1; 32]).public()
    }

    #[test]
    fn configured_relay_selects_relay_network() {
        assert_eq!(Network::from_relay(Some(relay())), Network::Relay(relay()));
    }

    #[test]
    fn no_relay_selects_n0_defaults() {
        assert_eq!(Network::from_relay(None), Network::N0);
    }

    #[test]
    fn relay_network_dials_server_through_the_relay() {
        let addr = Network::Relay(relay()).server_addr(laptop());

        assert_eq!(addr.id, laptop());
        assert_eq!(addr.relay_urls().collect::<Vec<_>>(), [&relay()]);
        assert_eq!(addr.ip_addrs().count(), 0);
    }

    #[test]
    fn n0_network_leaves_address_lookup_to_dns() {
        let addr = Network::N0.server_addr(laptop());

        assert_eq!(addr.id, laptop());
        assert!(addr.is_empty());
    }

    #[tokio::test]
    async fn binds_with_the_given_identity() {
        let secret = SecretKey::from_bytes(&[7; 32]);
        let expected = secret.public();

        let endpoint = Network::LocalOnly.bind(secret, vec![]).await.unwrap();

        assert_eq!(endpoint.id(), expected);
        endpoint.close().await;
    }
}
