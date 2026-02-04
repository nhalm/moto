//! ttyd WebSocket client for terminal access.
//!
//! This module provides a WebSocket client for connecting to ttyd terminals
//! running in garage pods. ttyd provides a web-based terminal interface that
//! we connect to via WebSocket for interactive terminal access.
//!
//! # Protocol
//!
//! ttyd uses a simple binary WebSocket protocol:
//! - First byte: message type (0=output, 1=input, 2=resize)
//! - Remaining bytes: payload
//!
//! # Example
//!
//! ```ignore
//! use moto_cli_wgtunnel::ttyd::{TtydClient, TtydConfig};
//!
//! let config = TtydConfig::new("fd00:6d6f:746f:1::1", 7681);
//! let client = TtydClient::new(config);
//! client.connect().await?;
//! ```

use std::io::{self, Write};
use std::net::Ipv6Addr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{self, disable_raw_mode, enable_raw_mode};
use futures_util::{SinkExt, StreamExt};
use thiserror::Error;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};
use tracing::{debug, info, warn};

use moto_wgtunnel_types::OverlayIp;

/// Default ttyd port.
pub const DEFAULT_TTYD_PORT: u16 = 7681;

/// ttyd message types.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TtydMessageType {
    /// Terminal output (server -> client).
    Output = 0,
    /// Terminal input (client -> server).
    Input = 1,
    /// Window resize (client -> server).
    Resize = 2,
    /// Ping/pong for keepalive.
    Ping = 3,
}

impl TryFrom<u8> for TtydMessageType {
    type Error = TtydError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Output),
            1 => Ok(Self::Input),
            2 => Ok(Self::Resize),
            3 => Ok(Self::Ping),
            _ => Err(TtydError::Protocol(format!(
                "unknown message type: {value}"
            ))),
        }
    }
}

/// Errors that can occur during ttyd operations.
#[derive(Debug, Error)]
pub enum TtydError {
    /// WebSocket connection failed.
    #[error("connection failed: {0}")]
    Connection(String),

    /// WebSocket error.
    #[error("websocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),

    /// Protocol error.
    #[error("protocol error: {0}")]
    Protocol(String),

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// Connection closed.
    #[error("connection closed")]
    Closed,

    /// Timeout waiting for connection.
    #[error("connection timeout")]
    Timeout,
}

/// Configuration for ttyd client.
#[derive(Debug, Clone)]
pub struct TtydConfig {
    /// Host to connect to (IPv6 overlay address).
    pub host: OverlayIp,

    /// Port (default: 7681).
    pub port: u16,

    /// Connection timeout in seconds.
    pub connect_timeout_secs: u64,
}

impl TtydConfig {
    /// Create a new ttyd configuration.
    #[must_use]
    pub const fn new(host: OverlayIp, port: u16) -> Self {
        Self {
            host,
            port,
            connect_timeout_secs: 30,
        }
    }

    /// Set the connection timeout.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn with_timeout(mut self, timeout_secs: u64) -> Self {
        self.connect_timeout_secs = timeout_secs;
        self
    }

    /// Get the WebSocket URL for this configuration.
    #[must_use]
    pub fn ws_url(&self) -> String {
        format!("ws://[{}]:{}/ws", self.host, self.port)
    }

    /// Get the socket address for TCP connection.
    fn socket_addr(&self) -> std::net::SocketAddrV6 {
        let ipv6: Ipv6Addr = self.host.into();
        std::net::SocketAddrV6::new(ipv6, self.port, 0, 0)
    }
}

/// ttyd WebSocket client.
///
/// Provides interactive terminal access to a garage via ttyd's WebSocket protocol.
pub struct TtydClient {
    config: TtydConfig,
}

impl TtydClient {
    /// Create a new ttyd client.
    #[must_use]
    pub const fn new(config: TtydConfig) -> Self {
        Self { config }
    }

    /// Connect to ttyd and run an interactive terminal session.
    ///
    /// This function takes over stdin/stdout for interactive terminal access.
    /// It returns when the connection is closed or an error occurs.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Connection fails
    /// - WebSocket handshake fails
    /// - Terminal I/O error occurs
    pub async fn run_interactive(&self) -> Result<(), TtydError> {
        info!(url = %self.config.ws_url(), "connecting to ttyd");

        // Connect to ttyd via TCP then upgrade to WebSocket
        let addr = self.config.socket_addr();
        let tcp_stream = tokio::time::timeout(
            std::time::Duration::from_secs(self.config.connect_timeout_secs),
            TcpStream::connect(addr),
        )
        .await
        .map_err(|_| TtydError::Timeout)?
        .map_err(|e| TtydError::Connection(format!("TCP connect failed: {e}")))?;

        debug!("TCP connection established, upgrading to WebSocket");

        // Upgrade to WebSocket
        let ws_url = self.config.ws_url();
        let (ws_stream, _response) = tokio_tungstenite::client_async(&ws_url, tcp_stream)
            .await
            .map_err(|e| TtydError::Connection(format!("WebSocket upgrade failed: {e}")))?;

        info!("WebSocket connection established");

        // Run the interactive session
        self.run_session(ws_stream).await
    }

    /// Run the interactive terminal session.
    async fn run_session(&self, ws_stream: WebSocketStream<TcpStream>) -> Result<(), TtydError> {
        let (mut ws_sink, mut ws_stream) = ws_stream.split();

        // Flag to signal shutdown
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        // Channel for terminal events
        let (event_tx, mut event_rx) = mpsc::channel::<Vec<u8>>(32);

        // Enable raw mode for the terminal
        enable_raw_mode()?;

        // Create RAII guard to restore terminal on exit
        let _raw_guard = RawModeGuard;

        // Send initial window size
        if let Ok((cols, rows)) = terminal::size() {
            let resize_msg = create_resize_message(cols, rows);
            ws_sink.send(Message::Binary(resize_msg.into())).await?;
            debug!(cols, rows, "sent initial window size");
        }

        // Spawn terminal event reader task
        let event_handle = tokio::task::spawn_blocking(move || {
            while running_clone.load(Ordering::Relaxed) {
                // Poll for events with a timeout
                if event::poll(std::time::Duration::from_millis(100)).unwrap_or(false) {
                    match event::read() {
                        Ok(Event::Key(key_event)) => {
                            if let Some(bytes) = key_event_to_bytes(key_event) {
                                if event_tx.blocking_send(bytes).is_err() {
                                    break;
                                }
                            }
                        }
                        Ok(Event::Resize(cols, rows)) => {
                            let resize_msg = create_resize_message(cols, rows);
                            if event_tx.blocking_send(resize_msg).is_err() {
                                break;
                            }
                        }
                        Ok(_) => {
                            // Ignore mouse events and other events
                        }
                        Err(_) => break,
                    }
                }
            }
        });

        // Main event loop
        let result = loop {
            tokio::select! {
                // Handle WebSocket messages from ttyd
                msg = ws_stream.next() => {
                    match msg {
                        Some(Ok(Message::Binary(data))) => {
                            if let Err(e) = self.handle_ttyd_message(&data) {
                                warn!(error = %e, "error handling ttyd message");
                            }
                        }
                        Some(Ok(Message::Text(text))) => {
                            // ttyd sometimes sends text for output
                            print!("{text}");
                            let _ = io::stdout().flush();
                        }
                        Some(Ok(Message::Close(_))) => {
                            debug!("ttyd closed connection");
                            break Ok(());
                        }
                        Some(Ok(Message::Ping(data))) => {
                            ws_sink.send(Message::Pong(data)).await?;
                        }
                        Some(Ok(_)) => {
                            // Ignore other message types
                        }
                        Some(Err(e)) => {
                            break Err(TtydError::WebSocket(e));
                        }
                        None => {
                            debug!("WebSocket stream ended");
                            break Ok(());
                        }
                    }
                }

                // Handle terminal input events
                Some(input) = event_rx.recv() => {
                    // Check if this is a resize message (starts with resize type)
                    if input.first() == Some(&(TtydMessageType::Resize as u8)) {
                        ws_sink.send(Message::Binary(input.into())).await?;
                    } else {
                        let msg = create_input_message(&input);
                        ws_sink.send(Message::Binary(msg.into())).await?;
                    }
                }
            }
        };

        // Signal event reader to stop
        running.store(false, Ordering::Relaxed);

        // Wait for event reader (with timeout)
        let _ = tokio::time::timeout(std::time::Duration::from_secs(1), event_handle).await;

        result
    }

    /// Handle a message from ttyd.
    #[allow(clippy::unused_self)]
    fn handle_ttyd_message(&self, data: &[u8]) -> Result<(), TtydError> {
        if data.is_empty() {
            return Ok(());
        }

        let msg_type = TtydMessageType::try_from(data[0])?;
        let payload = &data[1..];

        match msg_type {
            TtydMessageType::Output => {
                // Write terminal output to stdout
                let mut stdout = io::stdout();
                stdout.write_all(payload)?;
                stdout.flush()?;
            }
            TtydMessageType::Ping => {
                debug!("received ping from ttyd");
            }
            _ => {
                debug!(?msg_type, "received unexpected message type");
            }
        }

        Ok(())
    }
}

/// Convert a crossterm `KeyEvent` to bytes for ttyd.
fn key_event_to_bytes(event: KeyEvent) -> Option<Vec<u8>> {
    let bytes = match event.code {
        KeyCode::Char(c) => {
            if event.modifiers.contains(KeyModifiers::CONTROL) {
                // Ctrl+letter becomes control character (Ctrl+A = 0x01, etc.)
                let ctrl_char = (c.to_ascii_lowercase() as u8).wrapping_sub(b'a' - 1);
                vec![ctrl_char]
            } else if event.modifiers.contains(KeyModifiers::ALT) {
                // Alt+letter sends ESC followed by the character
                vec![0x1b, c as u8]
            } else {
                c.to_string().into_bytes()
            }
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => vec![0x1b, b'[', b'A'],
        KeyCode::Down => vec![0x1b, b'[', b'B'],
        KeyCode::Right => vec![0x1b, b'[', b'C'],
        KeyCode::Left => vec![0x1b, b'[', b'D'],
        KeyCode::Home => vec![0x1b, b'[', b'H'],
        KeyCode::End => vec![0x1b, b'[', b'F'],
        KeyCode::PageUp => vec![0x1b, b'[', b'5', b'~'],
        KeyCode::PageDown => vec![0x1b, b'[', b'6', b'~'],
        KeyCode::Insert => vec![0x1b, b'[', b'2', b'~'],
        KeyCode::Delete => vec![0x1b, b'[', b'3', b'~'],
        KeyCode::F(n) => {
            // F1-F12 escape sequences
            match n {
                1 => vec![0x1b, b'O', b'P'],
                2 => vec![0x1b, b'O', b'Q'],
                3 => vec![0x1b, b'O', b'R'],
                4 => vec![0x1b, b'O', b'S'],
                5 => vec![0x1b, b'[', b'1', b'5', b'~'],
                6 => vec![0x1b, b'[', b'1', b'7', b'~'],
                7 => vec![0x1b, b'[', b'1', b'8', b'~'],
                8 => vec![0x1b, b'[', b'1', b'9', b'~'],
                9 => vec![0x1b, b'[', b'2', b'0', b'~'],
                10 => vec![0x1b, b'[', b'2', b'1', b'~'],
                11 => vec![0x1b, b'[', b'2', b'3', b'~'],
                12 => vec![0x1b, b'[', b'2', b'4', b'~'],
                _ => return None,
            }
        }
        _ => return None,
    };

    Some(bytes)
}

/// Create an input message for ttyd.
fn create_input_message(input: &[u8]) -> Vec<u8> {
    let mut msg = Vec::with_capacity(1 + input.len());
    msg.push(TtydMessageType::Input as u8);
    msg.extend_from_slice(input);
    msg
}

/// Create a resize message for ttyd.
fn create_resize_message(cols: u16, rows: u16) -> Vec<u8> {
    // Format: type(1) + cols(2) + rows(2) in little-endian
    let mut msg = Vec::with_capacity(5);
    msg.push(TtydMessageType::Resize as u8);
    msg.extend_from_slice(&cols.to_le_bytes());
    msg.extend_from_slice(&rows.to_le_bytes());
    msg
}

/// RAII guard for raw terminal mode.
///
/// Restores the terminal to cooked mode when dropped.
struct RawModeGuard;

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        debug!("terminal mode restored");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ttyd_config_ws_url() {
        let host = OverlayIp::garage(1);
        let config = TtydConfig::new(host, 7681);
        assert!(config.ws_url().contains("7681"));
        assert!(config.ws_url().contains("/ws"));
    }

    #[test]
    fn ttyd_message_types() {
        assert_eq!(
            TtydMessageType::try_from(0).unwrap(),
            TtydMessageType::Output
        );
        assert_eq!(
            TtydMessageType::try_from(1).unwrap(),
            TtydMessageType::Input
        );
        assert_eq!(
            TtydMessageType::try_from(2).unwrap(),
            TtydMessageType::Resize
        );
        assert!(TtydMessageType::try_from(99).is_err());
    }

    #[test]
    fn create_input_message_format() {
        let msg = create_input_message(b"hello");
        assert_eq!(msg[0], TtydMessageType::Input as u8);
        assert_eq!(&msg[1..], b"hello");
    }

    #[test]
    fn create_resize_message_format() {
        let msg = create_resize_message(80, 24);
        assert_eq!(msg[0], TtydMessageType::Resize as u8);
        assert_eq!(msg.len(), 5);
        // Check little-endian encoding
        assert_eq!(u16::from_le_bytes([msg[1], msg[2]]), 80);
        assert_eq!(u16::from_le_bytes([msg[3], msg[4]]), 24);
    }

    #[test]
    fn key_event_to_bytes_char() {
        let event = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        let bytes = key_event_to_bytes(event).unwrap();
        assert_eq!(bytes, b"a");
    }

    #[test]
    fn key_event_to_bytes_ctrl() {
        let event = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        let bytes = key_event_to_bytes(event).unwrap();
        assert_eq!(bytes, vec![0x03]); // Ctrl+C
    }

    #[test]
    fn key_event_to_bytes_arrow() {
        let event = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        let bytes = key_event_to_bytes(event).unwrap();
        assert_eq!(bytes, vec![0x1b, b'[', b'A']);
    }
}
