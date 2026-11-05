use anyhow::{bail, Context, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const ALPN: &[u8] = b"clipsy/0";
pub const MAX_ENCODED_MESSAGE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Request {
    Write { content: String },
    Read,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Response {
    Written,
    Content { content: String },
    Denied,
    Failed { reason: String },
}

pub async fn write_message<T: Serialize>(
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
    serde_json::from_slice(&bytes).context("decoding message")
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

    const WRITE_WIRE: &str = r#"{"Write":{"content":"hi\n"}}"#;

    fn write_request() -> Request {
        Request::Write {
            content: "hi\n".into(),
        }
    }

    fn wire_requests() -> Vec<(&'static str, Request)> {
        vec![(r#""Read""#, Request::Read), (WRITE_WIRE, write_request())]
    }

    fn wire_responses() -> Vec<(&'static str, Response)> {
        vec![
            (r#""Written""#, Response::Written),
            (r#""Denied""#, Response::Denied),
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
            assert_eq!(encode(&request).await, wire);
            assert_eq!(decode::<Request>(wire).await.unwrap(), request);
        }
    }

    #[tokio::test]
    async fn responses_match_pinned_wire_format() {
        for (wire, response) in wire_responses() {
            assert_eq!(encode(&response).await, wire);
            assert_eq!(decode::<Response>(wire).await.unwrap(), response);
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
        };
        assert_eq!(round_trip(&request).await, request);
    }

    #[tokio::test]
    async fn round_trips_empty_content() {
        let request = Request::Write {
            content: String::new(),
        };
        assert_eq!(round_trip(&request).await, request);
    }

    #[tokio::test]
    async fn round_trips_over_a_stream() {
        let (mut client, mut server) = tokio::io::duplex(1024);
        let request = Request::Write {
            content: "x".repeat(10_000),
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
        let limit = WRITE_WIRE.len() as u64;

        let mut written = Vec::new();
        write_limited(&mut written, &write_request(), limit)
            .await
            .unwrap();
        let read: Request = read_limited(&mut WRITE_WIRE.as_bytes(), limit)
            .await
            .unwrap();

        assert_eq!(written, WRITE_WIRE.as_bytes());
        assert_eq!(read, write_request());
    }

    #[tokio::test]
    async fn refuses_to_send_message_one_byte_over_limit() {
        let mut written = Vec::new();

        let err = write_limited(&mut written, &write_request(), WRITE_WIRE.len() as u64 - 1)
            .await
            .unwrap_err();

        assert!(err.to_string().starts_with("outgoing message"), "{err}");
        assert!(written.is_empty());
    }

    #[tokio::test]
    async fn rejects_valid_message_one_byte_over_limit() {
        let err = read_limited::<Request>(&mut WRITE_WIRE.as_bytes(), WRITE_WIRE.len() as u64 - 1)
            .await
            .unwrap_err();

        assert!(err.to_string().starts_with("incoming message"), "{err}");
    }

    #[tokio::test]
    async fn rejects_garbage() {
        assert!(decode::<Request>("GET / HTTP/1.1\r\n\r\n").await.is_err());
    }

    #[tokio::test]
    async fn rejects_truncated_message() {
        assert!(decode::<Request>(r#"{"Write":{"content":"half"#)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn rejects_unknown_request() {
        assert!(decode::<Request>(r#""Delete""#).await.is_err());
    }
}
