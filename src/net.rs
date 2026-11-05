use iroh::endpoint::presets;
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, RelayUrl, SecretKey};

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

    pub async fn bind(&self, secret: SecretKey, alpns: Vec<Vec<u8>>) -> anyhow::Result<Endpoint> {
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
