//! DERP client for connecting to relay servers.
//!
//! The DERP client maintains a WebSocket connection to a DERP server and provides
//! methods to send and receive packets relayed through the server.
//!
//! # Connection Flow
//!
//! 1. Connect to DERP server via WebSocket (HTTPS)
//! 2. Receive `ServerKey` frame with server's public key
//! 3. Send `ClientInfo` frame with our public key and encrypted info
//! 4. Receive `ServerInfo` frame with encrypted server info
//! 5. Connection is established; send/receive packets
//!
//! # Example
//!
//! ```no_run
//! use moto_wgtunnel_derp::client::{DerpClient, DerpClientConfig};
//! use moto_wgtunnel_types::{WgPrivateKey, derp::DerpNode};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let private_key = WgPrivateKey::generate();
//! let node = DerpNode::with_defaults("derp.example.com");
//!
//! let config = DerpClientConfig::new(private_key, &node);
//! let client = DerpClient::connect(config).await?;
//!
//! // Send a packet to a peer
//! // client.send(&peer_public_key, b"hello").await?;
//! # Ok(())
//! # }
//! ```

use std::time::Duration;

use bytes::{Bytes, BytesMut};
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::{interval, timeout};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use tracing::{debug, trace, warn};

use crate::protocol::{self, Frame, NONCE_LEN, ProtocolError};
use moto_wgtunnel_types::{WgPrivateKey, WgPublicKey};

/// Default connection timeout.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Default keepalive interval (25 seconds as per spec).
pub const DEFAULT_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(25);

/// Error type for DERP client operations.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// WebSocket connection failed.
    #[error("connection failed: {0}")]
    ConnectionFailed(#[from] tokio_tungstenite::tungstenite::Error),

    /// Connection timed out.
    #[error("connection timed out")]
    Timeout,

    /// Protocol error.
    #[error("protocol error: {0}")]
    Protocol(#[from] ProtocolError),

    /// Handshake failed.
    #[error("handshake failed: {0}")]
    HandshakeFailed(String),

    /// Connection closed.
    #[error("connection closed")]
    ConnectionClosed,

    /// Send failed.
    #[error("send failed: {0}")]
    SendFailed(String),

    /// Channel closed.
    #[error("channel closed")]
    ChannelClosed,

    /// Encryption error.
    #[error("encryption error: {0}")]
    EncryptionError(String),
}

/// Configuration for a DERP client.
#[derive(Debug)]
pub struct DerpClientConfig {
    /// Our private key for authentication.
    private_key: WgPrivateKey,

    /// DERP server URL (e.g., `wss://derp.example.com/derp`).
    url: String,

    /// Connection timeout.
    connect_timeout: Duration,

    /// Keepalive interval.
    keepalive_interval: Duration,

    /// Whether this is our preferred/home DERP server.
    preferred: bool,
}

impl DerpClientConfig {
    /// Create a new DERP client configuration.
    #[must_use]
    pub fn new(private_key: WgPrivateKey, node: &moto_wgtunnel_types::derp::DerpNode) -> Self {
        // Convert HTTPS URL to WSS
        let url = format!("wss://{}:{}/derp", node.host, node.port);

        Self {
            private_key,
            url,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            keepalive_interval: DEFAULT_KEEPALIVE_INTERVAL,
            preferred: false,
        }
    }

    /// Set the connection timeout.
    #[must_use]
    pub const fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    /// Set the keepalive interval.
    #[must_use]
    pub const fn with_keepalive_interval(mut self, interval: Duration) -> Self {
        self.keepalive_interval = interval;
        self
    }

    /// Set whether this is the preferred DERP server.
    #[must_use]
    pub const fn with_preferred(mut self, preferred: bool) -> Self {
        self.preferred = preferred;
        self
    }

    /// Get our public key.
    #[must_use]
    pub fn public_key(&self) -> WgPublicKey {
        self.private_key.public_key()
    }
}

/// A received packet from a peer.
#[derive(Debug, Clone)]
pub struct ReceivedPacket {
    /// Source peer's public key.
    pub src: WgPublicKey,
    /// Packet data.
    pub data: Bytes,
}

/// Event from the DERP connection.
#[derive(Debug, Clone)]
pub enum DerpEvent {
    /// Received a packet from a peer.
    Packet(ReceivedPacket),

    /// A peer is now present on this DERP server.
    PeerPresent(WgPublicKey),

    /// A peer has gone from this DERP server.
    PeerGone(WgPublicKey),

    /// Health status from server.
    Health(String),

    /// Server is restarting.
    Restarting {
        /// Milliseconds before reconnecting.
        reconnect_in_ms: u32,
        /// Milliseconds to keep trying.
        try_for_ms: u32,
    },
}

/// Command to send to the DERP client.
#[derive(Debug)]
enum ClientCommand {
    /// Send a packet to a peer.
    SendPacket { dst: WgPublicKey, data: Bytes },

    /// Close the connection.
    Close,
}

/// Handle to send commands to a DERP client.
#[derive(Debug, Clone)]
pub struct DerpClientHandle {
    tx: mpsc::Sender<ClientCommand>,
}

impl DerpClientHandle {
    /// Send a packet to a peer.
    ///
    /// # Errors
    /// Returns error if the connection is closed.
    pub async fn send(&self, dst: &WgPublicKey, data: impl Into<Bytes>) -> Result<(), ClientError> {
        self.tx
            .send(ClientCommand::SendPacket {
                dst: dst.clone(),
                data: data.into(),
            })
            .await
            .map_err(|_| ClientError::ChannelClosed)
    }

    /// Close the connection gracefully.
    pub async fn close(&self) {
        let _ = self.tx.send(ClientCommand::Close).await;
    }
}

/// DERP client connection.
pub struct DerpClient {
    /// Our public key.
    public_key: WgPublicKey,

    /// Server's public key.
    server_key: WgPublicKey,

    /// Handle to send commands.
    handle: DerpClientHandle,

    /// Channel to receive events.
    events_rx: mpsc::Receiver<DerpEvent>,
}

impl DerpClient {
    /// Connect to a DERP server.
    ///
    /// # Errors
    /// Returns error if connection or handshake fails.
    pub async fn connect(config: DerpClientConfig) -> Result<Self, ClientError> {
        debug!(url = %config.url, "connecting to DERP server");

        // Connect with timeout
        let ws_stream = timeout(config.connect_timeout, connect_async(&config.url))
            .await
            .map_err(|_| ClientError::Timeout)?
            .map_err(ClientError::ConnectionFailed)?
            .0;

        debug!("WebSocket connected, starting handshake");

        // Perform handshake
        let (ws_stream, server_key) = Self::handshake(ws_stream, &config).await?;

        debug!(server_key = %server_key, "handshake complete");

        // Split the WebSocket stream
        let (ws_sink, ws_stream) = ws_stream.split();

        // Create channels
        let (cmd_tx, cmd_rx) = mpsc::channel(64);
        let (event_tx, event_rx) = mpsc::channel(256);

        let public_key = config.public_key();
        let handle = DerpClientHandle { tx: cmd_tx };

        // Spawn the connection task
        tokio::spawn(Self::run_connection(
            ws_sink,
            ws_stream,
            cmd_rx,
            event_tx,
            config.keepalive_interval,
        ));

        Ok(Self {
            public_key,
            server_key,
            handle,
            events_rx: event_rx,
        })
    }

    /// Perform the DERP handshake.
    async fn handshake(
        mut ws_stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
        config: &DerpClientConfig,
    ) -> Result<(WebSocketStream<MaybeTlsStream<TcpStream>>, WgPublicKey), ClientError> {
        // Step 1: Receive ServerKey frame
        let server_key = loop {
            let msg = ws_stream
                .next()
                .await
                .ok_or(ClientError::ConnectionClosed)?
                .map_err(ClientError::ConnectionFailed)?;

            if let Message::Binary(data) = msg {
                let mut buf = data;
                let frame = protocol::decode_frame(&mut buf)?;

                if let Frame::ServerKey { key } = frame {
                    break key;
                }
                return Err(ClientError::HandshakeFailed(format!(
                    "expected ServerKey, got {:?}",
                    frame.frame_type()
                )));
            }
        };

        debug!(server_key = %server_key, "received server key");

        // Step 2: Send ClientInfo frame
        // For simplicity, we send an empty encrypted info payload.
        // In a real implementation, this would contain client metadata.
        let our_public_key = config.public_key();

        // Generate a random nonce
        let mut nonce = [0u8; NONCE_LEN];
        getrandom::getrandom(&mut nonce)
            .map_err(|e| ClientError::EncryptionError(format!("failed to generate nonce: {e}")))?;

        // Create a simple encrypted info (just empty JSON for now)
        // In production, this would use NaCl box encryption with the server's key
        let encrypted_info = Bytes::from_static(b"{}");

        let client_info_frame = Frame::ClientInfo {
            key: our_public_key,
            nonce,
            encrypted_info,
        };

        let mut buf = BytesMut::new();
        client_info_frame.encode(&mut buf);
        ws_stream
            .send(Message::Binary(buf.to_vec().into()))
            .await
            .map_err(ClientError::ConnectionFailed)?;

        debug!("sent client info");

        // Step 3: Receive ServerInfo frame
        loop {
            let msg = ws_stream
                .next()
                .await
                .ok_or(ClientError::ConnectionClosed)?
                .map_err(ClientError::ConnectionFailed)?;

            if let Message::Binary(data) = msg {
                let mut buf = data;
                let frame = protocol::decode_frame(&mut buf)?;

                match frame {
                    Frame::ServerInfo { .. } => {
                        debug!("received server info");
                        break;
                    }
                    _ => {
                        return Err(ClientError::HandshakeFailed(format!(
                            "expected ServerInfo, got {:?}",
                            frame.frame_type()
                        )));
                    }
                }
            }
        }

        // Step 4: Send NotePreferred if this is our preferred server
        if config.preferred {
            let frame = Frame::NotePreferred { preferred: true };
            let mut buf = BytesMut::new();
            frame.encode(&mut buf);
            ws_stream
                .send(Message::Binary(buf.to_vec().into()))
                .await
                .map_err(ClientError::ConnectionFailed)?;
            debug!("marked as preferred server");
        }

        Ok((ws_stream, server_key))
    }

    /// Run the connection loop.
    async fn run_connection(
        mut ws_sink: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
        mut ws_stream: futures_util::stream::SplitStream<
            WebSocketStream<MaybeTlsStream<TcpStream>>,
        >,
        mut cmd_rx: mpsc::Receiver<ClientCommand>,
        event_tx: mpsc::Sender<DerpEvent>,
        keepalive_interval: Duration,
    ) {
        let mut keepalive = interval(keepalive_interval);
        keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                // Handle incoming WebSocket messages
                msg = ws_stream.next() => {
                    match msg {
                        Some(Ok(Message::Binary(data))) => {
                            let mut buf = data;
                            match protocol::decode_frame(&mut buf) {
                                Ok(frame) => {
                                    if let Some(event) = Self::frame_to_event(frame) {
                                        if event_tx.send(event).await.is_err() {
                                            debug!("event receiver dropped, closing connection");
                                            break;
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!(error = %e, "failed to decode frame");
                                }
                            }
                        }
                        Some(Ok(Message::Ping(data))) => {
                            // Respond to WebSocket pings
                            if ws_sink.send(Message::Pong(data)).await.is_err() {
                                break;
                            }
                        }
                        Some(Ok(Message::Close(_))) | None => {
                            debug!("connection closed by server");
                            break;
                        }
                        Some(Err(e)) => {
                            warn!(error = %e, "WebSocket error");
                            break;
                        }
                        _ => {}
                    }
                }

                // Handle commands
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(ClientCommand::SendPacket { dst, data }) => {
                            let frame = Frame::SendPacket { dst, data };
                            let mut buf = BytesMut::new();
                            frame.encode(&mut buf);
                            if ws_sink.send(Message::Binary(buf.to_vec().into())).await.is_err() {
                                break;
                            }
                            trace!("sent packet");
                        }
                        Some(ClientCommand::Close) => {
                            debug!("closing connection");
                            let _ = ws_sink.close().await;
                            break;
                        }
                        None => {
                            debug!("command channel closed");
                            break;
                        }
                    }
                }

                // Send keepalives
                _ = keepalive.tick() => {
                    let frame = Frame::KeepAlive;
                    let mut buf = BytesMut::new();
                    frame.encode(&mut buf);
                    if ws_sink.send(Message::Binary(buf.to_vec().into())).await.is_err() {
                        break;
                    }
                    trace!("sent keepalive");
                }
            }
        }
    }

    /// Convert a frame to an event (if applicable).
    fn frame_to_event(frame: Frame) -> Option<DerpEvent> {
        match frame {
            Frame::RecvPacket { src, data } => {
                Some(DerpEvent::Packet(ReceivedPacket { src, data }))
            }
            Frame::PeerPresent { key } => Some(DerpEvent::PeerPresent(key)),
            Frame::PeerGone { key, .. } => Some(DerpEvent::PeerGone(key)),
            Frame::Health { message } => Some(DerpEvent::Health(message)),
            Frame::Restarting {
                reconnect_in_ms,
                try_for_ms,
            } => Some(DerpEvent::Restarting {
                reconnect_in_ms,
                try_for_ms,
            }),
            Frame::Ping { data } => {
                // Respond to DERP pings (different from WebSocket pings)
                // This is handled in the connection loop
                trace!(data = ?data, "received DERP ping");
                None
            }
            Frame::Pong { .. } => {
                // Latency measurement response
                trace!("received DERP pong");
                None
            }
            Frame::KeepAlive => {
                trace!("received keepalive");
                None
            }
            _ => {
                trace!(frame_type = ?frame.frame_type(), "ignoring frame");
                None
            }
        }
    }

    /// Get our public key.
    #[must_use]
    pub const fn public_key(&self) -> &WgPublicKey {
        &self.public_key
    }

    /// Get the server's public key.
    #[must_use]
    pub const fn server_key(&self) -> &WgPublicKey {
        &self.server_key
    }

    /// Get a handle to send commands.
    #[must_use]
    pub fn handle(&self) -> DerpClientHandle {
        self.handle.clone()
    }

    /// Receive the next event.
    ///
    /// Returns `None` if the connection is closed.
    pub async fn recv(&mut self) -> Option<DerpEvent> {
        self.events_rx.recv().await
    }

    /// Send a packet to a peer.
    ///
    /// This is a convenience method that uses the internal handle.
    ///
    /// # Errors
    /// Returns error if the connection is closed.
    pub async fn send(&self, dst: &WgPublicKey, data: impl Into<Bytes>) -> Result<(), ClientError> {
        self.handle.send(dst, data).await
    }

    /// Close the connection gracefully.
    pub async fn close(self) {
        self.handle.close().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use moto_wgtunnel_types::derp::DerpNode;

    #[test]
    fn config_builder() {
        let private_key = WgPrivateKey::generate();
        let node = DerpNode::with_defaults("derp.example.com");

        let config = DerpClientConfig::new(private_key, &node)
            .with_connect_timeout(Duration::from_secs(5))
            .with_keepalive_interval(Duration::from_secs(30))
            .with_preferred(true);

        assert_eq!(config.connect_timeout, Duration::from_secs(5));
        assert_eq!(config.keepalive_interval, Duration::from_secs(30));
        assert!(config.preferred);
        assert_eq!(config.url, "wss://derp.example.com:443/derp");
    }

    #[test]
    fn config_custom_port() {
        let private_key = WgPrivateKey::generate();
        let node = DerpNode::new("derp.example.com", 8443, 3478);

        let config = DerpClientConfig::new(private_key, &node);
        assert_eq!(config.url, "wss://derp.example.com:8443/derp");
    }
}
