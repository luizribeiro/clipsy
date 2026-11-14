use anyhow::{bail, Context, Result};
use serde::{
    de::{DeserializeOwned, IgnoredAny},
    Deserialize, Serialize,
};
use serde_json::value::RawValue;
use std::fmt;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const ALPN: &[u8] = b"clipsy/0";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const MAX_ENCODED_MESSAGE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Request {
    Write {
        content: String,
        #[serde(default)]
        concealed: bool,
    },
    Read,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Response {
    Written,
    Content { content: String },
    Denied,
    Concealed,
    Failed { reason: String },
}

#[derive(Serialize)]
struct Outgoing<'a, T> {
    clipsy: &'a str,
    message: &'a T,
}

#[derive(Deserialize)]
struct Incoming {
    clipsy: String,
    message: Box<RawValue>,
}

/// A message from a peer running a different clipsy that this one can't decode.
#[derive(Debug, PartialEq, Eq)]
pub enum Incompatible {
    Unversioned,
    Version { peer: String, detail: String },
}

impl fmt::Display for Incompatible {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unversioned => write!(
                f,
                "the other side sent no clipsy version, so it likely runs clipsy 0.2 or older; upgrade it to clipsy {VERSION}"
            ),
            Self::Version { peer, detail } => write!(
                f,
                "the other side runs clipsy {peer} and this is clipsy {VERSION}; run the same version on both ({detail})"
            ),
        }
    }
}

impl std::error::Error for Incompatible {}

pub async fn write_message<T: Serialize>(
    writer: &mut (impl AsyncWrite + Unpin),
    message: &T,
) -> Result<()> {
    let versioned = Outgoing {
        clipsy: VERSION,
        message,
    };
    write_limited(writer, &versioned, MAX_ENCODED_MESSAGE_BYTES).await
}

/// Replies in the format clipsy 0.2 and older read, so they can show why
/// they were turned away.
pub async fn write_unversioned<T: Serialize>(
    writer: &mut (impl AsyncWrite + Unpin),
    message: &T,
) -> Result<()> {
    write_limited(writer, message, MAX_ENCODED_MESSAGE_BYTES).await
}

pub async fn read_message<T: DeserializeOwned>(reader: &mut (impl AsyncRead + Unpin)) -> Result<T> {
    read_limited(reader, MAX_ENCODED_MESSAGE_BYTES).await
}

async fn write_limited<T: Serialize>(
    writer: &mut (impl AsyncWrite + Unpin),
    message: &T,
    limit: u64,
) -> Result<()> {
    let bytes = serde_json::to_vec(message)?;
    if bytes.len() as u64 > limit {
        bail!(
            "outgoing message is {} bytes, limit is {limit}",
            bytes.len()
        );
    }
    writer.write_all(&bytes).await.context("sending message")?;
    Ok(())
}

async fn read_limited<T: DeserializeOwned>(
    reader: &mut (impl AsyncRead + Unpin),
    limit: u64,
) -> Result<T> {
    let mut bytes = Vec::new();
    reader
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .await
        .context("receiving message")?;
    if bytes.len() as u64 > limit {
        bail!("incoming message exceeds the {limit} byte limit");
    }
    decode_versioned(&bytes)
}

fn decode_versioned<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    let incoming: Incoming = match serde_json::from_slice(bytes) {
        Ok(incoming) => incoming,
        Err(_) if serde_json::from_slice::<IgnoredAny>(bytes).is_ok() => {
            return Err(Incompatible::Unversioned.into())
        }
        Err(e) => return Err(anyhow::Error::new(e).context("decoding message")),
    };
    serde_json::from_str(incoming.message.get()).map_err(|e| {
        if incoming.clipsy == VERSION {
            anyhow::Error::new(e).context("decoding message")
        } else {
            Incompatible::Version {
                peer: incoming.clipsy,
                detail: e.to_string(),
            }
            .into()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn encode<T: Serialize>(message: &T) -> String {
        let mut bytes = Vec::new();
        write_message(&mut bytes, message).await.unwrap();
        String::from_utf8(bytes).unwrap()
    }

    async fn decode<T: DeserializeOwned>(text: &str) -> Result<T> {
        read_message(&mut text.as_bytes()).await
    }

    async fn round_trip<T: Serialize + DeserializeOwned>(message: &T) -> T {
        decode(&encode(message).await).await.unwrap()
    }

    const WRITE_WIRE: &str = r#"{"Write":{"content":"hi\n","concealed":false}}"#;

    fn versioned(message: &Request) -> Outgoing<'_, Request> {
        Outgoing {
            clipsy: VERSION,
            message,
        }
    }

    fn wire(version: &str, message: &str) -> String {
        format!(r#"{{"clipsy":"{version}","message":{message}}}"#)
    }

    fn current(message: &str) -> String {
        wire(VERSION, message)
    }

    fn write_request() -> Request {
        Request::Write {
            content: "hi\n".into(),
            concealed: false,
        }
    }

    fn wire_requests() -> Vec<(&'static str, Request)> {
        vec![
            (r#""Read""#, Request::Read),
            (WRITE_WIRE, write_request()),
            (
                r#"{"Write":{"content":"hunter2","concealed":true}}"#,
                Request::Write {
                    content: "hunter2".into(),
                    concealed: true,
                },
            ),
        ]
    }

    fn wire_responses() -> Vec<(&'static str, Response)> {
        vec![
            (r#""Written""#, Response::Written),
            (r#""Denied""#, Response::Denied),
            (r#""Concealed""#, Response::Concealed),
            (
                r#"{"Content":{"content":"hi"}}"#,
                Response::Content {
                    content: "hi".into(),
                },
            ),
            (
                r#"{"Failed":{"reason":"no text"}}"#,
                Response::Failed {
                    reason: "no text".into(),
                },
            ),
        ]
    }

    #[tokio::test]
    async fn requests_match_pinned_wire_format() {
        for (wire, request) in wire_requests() {
            assert_eq!(encode(&request).await, current(wire));
            assert_eq!(decode::<Request>(&current(wire)).await.unwrap(), request);
        }
    }

    #[tokio::test]
    async fn responses_match_pinned_wire_format() {
        for (wire, response) in wire_responses() {
            assert_eq!(encode(&response).await, current(wire));
            assert_eq!(decode::<Response>(&current(wire)).await.unwrap(), response);
        }
    }

    #[tokio::test]
    async fn round_trips_unicode_and_control_characters() {
        let response = Response::Content {
            content: "héllo 📋\r\n\t\"quoted\" \\ \u{0}".into(),
        };
        assert_eq!(round_trip(&response).await, response);
    }

    #[tokio::test]
    async fn round_trips_large_content() {
        let request = Request::Write {
            content: "fn main() {}\n".repeat(50_000),
            concealed: false,
        };
        assert_eq!(round_trip(&request).await, request);
    }

    #[tokio::test]
    async fn round_trips_empty_content() {
        let request = Request::Write {
            content: String::new(),
            concealed: false,
        };
        assert_eq!(round_trip(&request).await, request);
    }

    #[tokio::test]
    async fn round_trips_over_a_stream() {
        let (mut client, mut server) = tokio::io::duplex(1024);
        let request = Request::Write {
            content: "x".repeat(10_000),
            concealed: false,
        };
        let send = async {
            write_message(&mut client, &request).await.unwrap();
            drop(client);
        };
        let (_, received) = tokio::join!(send, read_message::<Request>(&mut server));
        assert_eq!(received.unwrap(), request);
    }

    #[tokio::test]
    async fn accepts_message_exactly_at_limit() {
        let wire = current(WRITE_WIRE);
        let limit = wire.len() as u64;

        let mut written = Vec::new();
        write_limited(&mut written, &versioned(&write_request()), limit)
            .await
            .unwrap();
        let read: Request = read_limited(&mut wire.as_bytes(), limit).await.unwrap();

        assert_eq!(written, wire.as_bytes());
        assert_eq!(read, write_request());
    }

    #[tokio::test]
    async fn size_limit_counts_the_version_envelope() {
        let mut written = Vec::new();
        let fits_only_bare = WRITE_WIRE.len() as u64;

        let err = write_limited(&mut written, &versioned(&write_request()), fits_only_bare)
            .await
            .unwrap_err();

        assert!(err.to_string().starts_with("outgoing message"), "{err}");
        assert!(written.is_empty());
    }

    #[tokio::test]
    async fn rejects_valid_message_one_byte_over_limit() {
        let wire = current(WRITE_WIRE);
        let err = read_limited::<Request>(&mut wire.as_bytes(), wire.len() as u64 - 1)
            .await
            .unwrap_err();

        assert!(err.to_string().starts_with("incoming message"), "{err}");
    }

    #[tokio::test]
    async fn non_json_is_a_plain_decode_error() {
        let truncated = current(r#"{"Write":{"content":"half"#);
        for wire in ["GET / HTTP/1.1\r\n\r\n", "", &truncated] {
            let err = decode::<Request>(wire).await.unwrap_err();

            assert!(err.downcast_ref::<Incompatible>().is_none(), "{err:#}");
            assert!(
                format!("{err:#}").starts_with("decoding message"),
                "{err:#}"
            );
        }
    }

    fn incompatibility(err: anyhow::Error) -> Incompatible {
        err.downcast().expect("an incompatibility")
    }

    #[tokio::test]
    async fn unknown_message_from_the_same_version_is_a_plain_decode_error() {
        let err = decode::<Request>(&current(r#""Delete""#))
            .await
            .unwrap_err();

        assert!(err.downcast_ref::<Incompatible>().is_none(), "{err:#}");
        assert!(
            format!("{err:#}").contains("unknown variant `Delete`"),
            "{err:#}"
        );
    }

    #[tokio::test]
    async fn unknown_message_from_another_version_names_both_versions() {
        let err = decode::<Response>(&wire("9.1.0", r#""Archived""#))
            .await
            .unwrap_err();
        let shown = err.to_string();

        assert!(
            matches!(incompatibility(err), Incompatible::Version { peer, .. } if peer == "9.1.0")
        );
        assert!(shown.contains("clipsy 9.1.0"), "{shown}");
        assert!(
            shown.contains(&format!("this is clipsy {VERSION}")),
            "{shown}"
        );
        assert!(shown.contains("unknown variant `Archived`"), "{shown}");
    }

    #[tokio::test]
    async fn known_message_from_another_version_is_accepted() {
        let request = decode::<Request>(&wire("9.1.0", r#""Read""#)).await;

        assert_eq!(request.unwrap(), Request::Read);
    }

    #[tokio::test]
    async fn messages_from_clipsy_0_2_are_unversioned() {
        for wire in [
            r#""Read""#,
            WRITE_WIRE,
            r#"{"Failed":{"reason":"decoding message"}}"#,
        ] {
            let err = decode::<Request>(wire).await.unwrap_err();
            let shown = err.to_string();

            assert_eq!(incompatibility(err), Incompatible::Unversioned, "{wire}");
            assert!(shown.contains("clipsy 0.2 or older"), "{shown}");
            assert!(
                shown.contains(&format!("upgrade it to clipsy {VERSION}")),
                "{shown}"
            );
        }
    }

    #[tokio::test]
    async fn unversioned_replies_use_the_bare_format() {
        let mut bytes = Vec::new();
        write_unversioned(&mut bytes, &Response::Denied)
            .await
            .unwrap();

        assert_eq!(bytes, br#""Denied""#);
    }

    #[tokio::test]
    async fn malformed_envelopes_are_unversioned() {
        for wire in [
            r#"{"clipsy":1,"message":"Read"}"#,
            r#"{"clipsy":"0.3.0"}"#,
            "42",
        ] {
            let err = decode::<Request>(wire).await.unwrap_err();

            assert_eq!(incompatibility(err), Incompatible::Unversioned, "{wire}");
        }
    }
}
