//! The mTLS session inside a WebSocket on the panel's HTTPS port.
//!
//! Panels reachable only through a Cloudflare Tunnel or another HTTPS reverse
//! proxy cannot offer the raw gRPC listener port. The agent then upgrades a
//! WebSocket on the machine-client prefix and runs the same TLS handshake and
//! gRPC inside it, so the proxy only ever sees ordinary WebSocket traffic.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use anyhow::Context as _;
use bytes::{Buf, Bytes};
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tonic::transport::{Channel, Endpoint, Uri};

/// The machine-client prefix the panel routes to the API, and the only path
/// family Cloudflare Access's agent bypass covers at the edge. Using it for
/// enrollment and the tunnel keeps machine traffic working when the panel
/// domain is protected by Zero Trust and the operator allowed agents through.
const TUNNEL_PATH: &str = "/mcp/api/v1/agent/tunnel";

/// `ws(s)://{panel}/mcp/api/v1/agent/tunnel` for the stored panel URL.
pub fn url(panel_url: &str) -> anyhow::Result<String> {
    let mut url = reqwest::Url::parse(panel_url).context("the stored panel URL is invalid")?;
    let scheme = match url.scheme() {
        "https" => "wss",
        "http" => "ws",
        other => anyhow::bail!("the panel URL scheme '{other}' cannot carry a WebSocket"),
    };
    url.set_scheme(scheme)
        .map_err(|()| anyhow::anyhow!("cannot switch the panel URL to {scheme}"))?;
    url.set_path(TUNNEL_PATH);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.into())
}

/// Open the WebSocket and let tonic run the mTLS handshake over it. The
/// endpoint's TLS configuration is applied by tonic, exactly like a direct
/// connection.
pub async fn connect(endpoint: &Endpoint, url: &str) -> anyhow::Result<Channel> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let url = url.to_owned();
    let inner = url.clone();
    let connector = tower::service_fn(move |_uri: Uri| {
        let url = inner.clone();
        async move {
            let (socket, _response) = tokio_tungstenite::connect_async(&url)
                .await
                .with_context(|| format!("open {url}"))?;
            Ok::<_, anyhow::Error>(hyper_util::rt::TokioIo::new(WsIo::new(socket)))
        }
    });
    endpoint
        .connect_with_connector(connector)
        .await
        .map_err(|error| anyhow::anyhow!(error).context(format!("connect {url}")))
}

/// A WebSocket as a byte stream: binary messages carry the inner TLS records.
struct WsIo {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    /// Unread remainder of the last received message.
    pending: Bytes,
}

impl WsIo {
    fn new(socket: WebSocketStream<MaybeTlsStream<TcpStream>>) -> Self {
        Self {
            socket,
            pending: Bytes::new(),
        }
    }
}

impl AsyncRead for WsIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        while self.pending.is_empty() {
            match self.socket.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok(Message::Binary(bytes)))) => self.pending = bytes,
                Poll::Ready(Some(Ok(Message::Text(text)))) => {
                    self.pending = Bytes::copy_from_slice(text.as_bytes());
                }
                Poll::Ready(Some(Ok(Message::Close(_)))) | Poll::Ready(None) => {
                    return Poll::Ready(Ok(()));
                }
                Poll::Ready(Some(Ok(_))) => continue,
                Poll::Ready(Some(Err(error))) => return Poll::Ready(Err(io_error(error))),
                Poll::Pending => return Poll::Pending,
            }
        }
        let take = self.pending.len().min(buf.remaining());
        buf.put_slice(&self.pending[..take]);
        self.pending.advance(take);
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for WsIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.socket.poll_ready_unpin(cx) {
            Poll::Ready(Ok(())) => self
                .socket
                .start_send_unpin(Message::Binary(Bytes::copy_from_slice(buf)))
                .map(|()| buf.len())
                .map_err(io_error)
                .into(),
            Poll::Ready(Err(error)) => Poll::Ready(Err(io_error(error))),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.socket.poll_flush_unpin(cx).map_err(io_error)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.socket.poll_close_unpin(cx).map_err(io_error)
    }
}

fn io_error(error: tokio_tungstenite::tungstenite::Error) -> io::Error {
    match error {
        tokio_tungstenite::tungstenite::Error::Io(inner) => inner,
        other => io::Error::other(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tunnel_url_switches_the_scheme_and_path() {
        assert_eq!(
            url("https://panel.example.com").unwrap(),
            "wss://panel.example.com/mcp/api/v1/agent/tunnel"
        );
        assert_eq!(
            url("http://127.0.0.1:3000/").unwrap(),
            "ws://127.0.0.1:3000/mcp/api/v1/agent/tunnel"
        );
        assert!(url("ftp://panel.example.com").is_err());
    }
}
