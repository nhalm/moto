//! STUN (Session Traversal Utilities for NAT) client for NAT discovery.
//!
//! STUN is used to discover the public IP address and port mapping of a client
//! behind NAT. This information is used to establish direct `WireGuard`
//! connections when possible.
//!
//! # Protocol Overview
//!
//! STUN uses a simple request/response protocol over UDP:
//! 1. Client sends a Binding Request to a STUN server
//! 2. Server responds with a Binding Response containing the client's
//!    reflexive transport address (public IP:port as seen by the server)
//!
//! # Message Format
//!
//! ```text
//! 0                   1                   2                   3
//! 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |0 0|     STUN Message Type     |         Message Length        |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |                         Magic Cookie                          |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |                                                               |
//! |                     Transaction ID (96 bits)                  |
//! |                                                               |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! ```
//!
//! # Example
//!
//! ```no_run
//! use moto_wgtunnel_conn::stun::{StunClient, StunResult};
//! use std::net::SocketAddr;
//! use std::time::Duration;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let client = StunClient::new();
//!
//! // Discover public endpoint via STUN server
//! let stun_server: SocketAddr = "stun.example.com:3478".parse()?;
//! let result = client.discover(stun_server, Duration::from_secs(3)).await?;
//!
//! println!("Public endpoint: {}", result.reflexive_addr);
//! # Ok(())
//! # }
//! ```
//!
//! # References
//!
//! - [RFC 5389](https://tools.ietf.org/html/rfc5389) - STUN protocol

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::time::Duration;

use thiserror::Error;
use tokio::net::UdpSocket;
use tokio::time::timeout;
use tracing::{debug, trace};

/// STUN magic cookie (RFC 5389).
pub const MAGIC_COOKIE: u32 = 0x2112_A442;

/// STUN header length in bytes.
pub const HEADER_LEN: usize = 20;

/// STUN transaction ID length in bytes.
pub const TRANSACTION_ID_LEN: usize = 12;

/// Maximum STUN message size.
pub const MAX_MESSAGE_SIZE: usize = 548;

/// Default STUN timeout.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);

/// Default number of retries.
pub const DEFAULT_RETRIES: u32 = 2;

/// STUN message types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum MessageType {
    /// Binding Request (0x0001).
    BindingRequest = 0x0001,
    /// Binding Response (0x0101).
    BindingResponse = 0x0101,
    /// Binding Error Response (0x0111).
    BindingErrorResponse = 0x0111,
}

impl MessageType {
    /// Parse message type from u16.
    #[must_use]
    pub const fn from_u16(value: u16) -> Option<Self> {
        match value {
            0x0001 => Some(Self::BindingRequest),
            0x0101 => Some(Self::BindingResponse),
            0x0111 => Some(Self::BindingErrorResponse),
            _ => None,
        }
    }
}

/// STUN attribute types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum AttributeType {
    /// MAPPED-ADDRESS (0x0001) - deprecated but still used.
    MappedAddress = 0x0001,
    /// XOR-MAPPED-ADDRESS (0x0020) - preferred.
    XorMappedAddress = 0x0020,
    /// ERROR-CODE (0x0009).
    ErrorCode = 0x0009,
    /// SOFTWARE (0x8022).
    Software = 0x8022,
}

impl AttributeType {
    /// Parse attribute type from u16.
    #[must_use]
    pub const fn from_u16(value: u16) -> Option<Self> {
        match value {
            0x0001 => Some(Self::MappedAddress),
            0x0020 => Some(Self::XorMappedAddress),
            0x0009 => Some(Self::ErrorCode),
            0x8022 => Some(Self::Software),
            _ => None,
        }
    }
}

/// Errors that can occur during STUN operations.
#[derive(Debug, Error)]
pub enum StunError {
    /// I/O error during socket operations.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// STUN request timed out.
    #[error("STUN request timed out after {0:?}")]
    Timeout(Duration),

    /// Invalid STUN message format.
    #[error("Invalid STUN message: {0}")]
    InvalidMessage(String),

    /// Transaction ID mismatch.
    #[error("Transaction ID mismatch")]
    TransactionIdMismatch,

    /// No mapped address in response.
    #[error("No mapped address in STUN response")]
    NoMappedAddress,

    /// STUN error response received.
    #[error("STUN error response: {code} {reason}")]
    ErrorResponse {
        /// Error code.
        code: u16,
        /// Error reason phrase.
        reason: String,
    },
}

/// Result of a successful STUN discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StunResult {
    /// The reflexive transport address (public IP:port as seen by STUN server).
    pub reflexive_addr: SocketAddr,

    /// The local address used for the STUN request.
    pub local_addr: SocketAddr,
}

/// A STUN client for NAT discovery.
///
/// The client sends STUN Binding Requests to discover the public IP address
/// and port mapping of the local host.
#[derive(Debug, Default)]
pub struct StunClient {
    _private: (),
}

impl StunClient {
    /// Create a new STUN client.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Discover the public endpoint via a STUN server.
    ///
    /// Sends a STUN Binding Request and returns the reflexive transport address
    /// from the response.
    ///
    /// # Arguments
    /// - `server`: The STUN server address
    /// - `timeout_duration`: Maximum time to wait for response
    ///
    /// # Errors
    /// Returns an error if the request times out, the response is invalid,
    /// or the server returns an error.
    pub async fn discover(
        &self,
        server: SocketAddr,
        timeout_duration: Duration,
    ) -> Result<StunResult, StunError> {
        self.discover_with_retries(server, timeout_duration, DEFAULT_RETRIES)
            .await
    }

    /// Discover the public endpoint with configurable retries.
    ///
    /// # Arguments
    /// - `server`: The STUN server address
    /// - `timeout_duration`: Maximum time to wait for each attempt
    /// - `retries`: Number of retry attempts (0 = single attempt)
    ///
    /// # Errors
    /// Returns an error if all attempts fail.
    pub async fn discover_with_retries(
        &self,
        server: SocketAddr,
        timeout_duration: Duration,
        retries: u32,
    ) -> Result<StunResult, StunError> {
        let mut last_error = None;

        for attempt in 0..=retries {
            if attempt > 0 {
                debug!(attempt, "Retrying STUN request");
            }

            match self.discover_once(server, timeout_duration).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    debug!(attempt, error = %e, "STUN request failed");
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or(StunError::Timeout(timeout_duration)))
    }

    /// Single STUN discovery attempt.
    async fn discover_once(
        &self,
        server: SocketAddr,
        timeout_duration: Duration,
    ) -> Result<StunResult, StunError> {
        // Bind to appropriate address family
        let bind_addr: SocketAddr = match server {
            SocketAddr::V4(_) => SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)),
            SocketAddr::V6(_) => SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 0, 0, 0)),
        };

        let socket = UdpSocket::bind(bind_addr).await?;
        let local_addr = socket.local_addr()?;

        debug!(%server, %local_addr, "Sending STUN binding request");

        // Generate transaction ID
        let transaction_id = generate_transaction_id();

        // Build and send request
        let request = build_binding_request(&transaction_id);
        socket.send_to(&request, server).await?;

        // Wait for response with timeout
        let mut buf = [0u8; MAX_MESSAGE_SIZE];
        let (len, from) = timeout(timeout_duration, socket.recv_from(&mut buf))
            .await
            .map_err(|_| StunError::Timeout(timeout_duration))??;

        trace!(%from, len, "Received STUN response");

        // Parse response
        let response = &buf[..len];
        let reflexive_addr = parse_binding_response(response, &transaction_id)?;

        debug!(%reflexive_addr, "STUN discovery successful");

        Ok(StunResult {
            reflexive_addr,
            local_addr,
        })
    }
}

/// Generate a random 12-byte transaction ID.
fn generate_transaction_id() -> [u8; TRANSACTION_ID_LEN] {
    let mut id = [0u8; TRANSACTION_ID_LEN];
    rand::fill(&mut id);
    id
}

/// Build a STUN Binding Request message.
fn build_binding_request(transaction_id: &[u8; TRANSACTION_ID_LEN]) -> [u8; HEADER_LEN] {
    let mut msg = [0u8; HEADER_LEN];

    // Message type: Binding Request (0x0001)
    msg[0] = 0x00;
    msg[1] = 0x01;

    // Message length: 0 (no attributes)
    msg[2] = 0x00;
    msg[3] = 0x00;

    // Magic cookie
    msg[4..8].copy_from_slice(&MAGIC_COOKIE.to_be_bytes());

    // Transaction ID
    msg[8..20].copy_from_slice(transaction_id);

    msg
}

/// Parse a STUN Binding Response and extract the reflexive address.
fn parse_binding_response(
    data: &[u8],
    expected_transaction_id: &[u8; TRANSACTION_ID_LEN],
) -> Result<SocketAddr, StunError> {
    if data.len() < HEADER_LEN {
        return Err(StunError::InvalidMessage("Message too short".to_string()));
    }

    // Check message type
    let msg_type = u16::from_be_bytes([data[0], data[1]]);
    let msg_type = MessageType::from_u16(msg_type).ok_or_else(|| {
        StunError::InvalidMessage(format!("Unknown message type: {msg_type:#06x}"))
    })?;

    if msg_type == MessageType::BindingErrorResponse {
        // Parse error response
        let (code, reason) = parse_error_code(&data[HEADER_LEN..]);
        return Err(StunError::ErrorResponse { code, reason });
    }

    if msg_type != MessageType::BindingResponse {
        return Err(StunError::InvalidMessage(format!(
            "Expected Binding Response, got {msg_type:?}"
        )));
    }

    // Check magic cookie
    let cookie = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    if cookie != MAGIC_COOKIE {
        return Err(StunError::InvalidMessage(
            "Invalid magic cookie".to_string(),
        ));
    }

    // Verify transaction ID
    if &data[8..20] != expected_transaction_id {
        return Err(StunError::TransactionIdMismatch);
    }

    // Parse message length
    let msg_len = u16::from_be_bytes([data[2], data[3]]) as usize;
    if data.len() < HEADER_LEN + msg_len {
        return Err(StunError::InvalidMessage(
            "Message shorter than declared length".to_string(),
        ));
    }

    // Parse attributes
    let attrs = &data[HEADER_LEN..HEADER_LEN + msg_len];
    parse_mapped_address(attrs, &data[4..8])
}

/// Parse attributes looking for XOR-MAPPED-ADDRESS or MAPPED-ADDRESS.
fn parse_mapped_address(mut attrs: &[u8], magic_cookie: &[u8]) -> Result<SocketAddr, StunError> {
    let mut mapped_addr = None;
    let mut xor_mapped_addr = None;

    while attrs.len() >= 4 {
        let attr_type = u16::from_be_bytes([attrs[0], attrs[1]]);
        let attr_len = u16::from_be_bytes([attrs[2], attrs[3]]) as usize;

        // Check we have enough data
        if attrs.len() < 4 + attr_len {
            break;
        }

        let attr_value = &attrs[4..4 + attr_len];

        match AttributeType::from_u16(attr_type) {
            Some(AttributeType::XorMappedAddress) => {
                xor_mapped_addr = parse_xor_mapped_address_value(attr_value, magic_cookie).ok();
            }
            Some(AttributeType::MappedAddress) => {
                mapped_addr = parse_mapped_address_value(attr_value).ok();
            }
            _ => {
                // Skip unknown attributes
            }
        }

        // Move to next attribute (padded to 4-byte boundary)
        let padded_len = (attr_len + 3) & !3;
        if attrs.len() < 4 + padded_len {
            break;
        }
        attrs = &attrs[4 + padded_len..];
    }

    // Prefer XOR-MAPPED-ADDRESS over MAPPED-ADDRESS
    xor_mapped_addr
        .or(mapped_addr)
        .ok_or(StunError::NoMappedAddress)
}

/// Parse XOR-MAPPED-ADDRESS attribute value.
fn parse_xor_mapped_address_value(
    value: &[u8],
    magic_cookie: &[u8],
) -> Result<SocketAddr, StunError> {
    if value.len() < 8 {
        return Err(StunError::InvalidMessage(
            "XOR-MAPPED-ADDRESS too short".to_string(),
        ));
    }

    let family = value[1];
    let x_port = u16::from_be_bytes([value[2], value[3]]);
    let port = x_port ^ ((MAGIC_COOKIE >> 16) as u16);

    match family {
        0x01 => {
            // IPv4
            if value.len() < 8 {
                return Err(StunError::InvalidMessage(
                    "XOR-MAPPED-ADDRESS IPv4 too short".to_string(),
                ));
            }
            let x_addr = [value[4], value[5], value[6], value[7]];
            let addr = [
                x_addr[0] ^ magic_cookie[0],
                x_addr[1] ^ magic_cookie[1],
                x_addr[2] ^ magic_cookie[2],
                x_addr[3] ^ magic_cookie[3],
            ];
            Ok(SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::from(addr),
                port,
            )))
        }
        0x02 => {
            // IPv6
            if value.len() < 20 {
                return Err(StunError::InvalidMessage(
                    "XOR-MAPPED-ADDRESS IPv6 too short".to_string(),
                ));
            }
            // IPv6 XOR includes transaction ID, but we use only the magic cookie portion
            // for the first 4 bytes. Full implementation would need the transaction ID.
            // For simplicity, we fall back to MAPPED-ADDRESS for IPv6.
            Err(StunError::InvalidMessage(
                "IPv6 XOR-MAPPED-ADDRESS not fully implemented".to_string(),
            ))
        }
        _ => Err(StunError::InvalidMessage(format!(
            "Unknown address family: {family:#04x}"
        ))),
    }
}

/// Parse MAPPED-ADDRESS attribute value.
fn parse_mapped_address_value(value: &[u8]) -> Result<SocketAddr, StunError> {
    if value.len() < 8 {
        return Err(StunError::InvalidMessage(
            "MAPPED-ADDRESS too short".to_string(),
        ));
    }

    let family = value[1];
    let port = u16::from_be_bytes([value[2], value[3]]);

    match family {
        0x01 => {
            // IPv4
            let addr = Ipv4Addr::new(value[4], value[5], value[6], value[7]);
            Ok(SocketAddr::V4(SocketAddrV4::new(addr, port)))
        }
        0x02 => {
            // IPv6
            if value.len() < 20 {
                return Err(StunError::InvalidMessage(
                    "MAPPED-ADDRESS IPv6 too short".to_string(),
                ));
            }
            let mut addr_bytes = [0u8; 16];
            addr_bytes.copy_from_slice(&value[4..20]);
            let addr = Ipv6Addr::from(addr_bytes);
            Ok(SocketAddr::V6(SocketAddrV6::new(addr, port, 0, 0)))
        }
        _ => Err(StunError::InvalidMessage(format!(
            "Unknown address family: {family:#04x}"
        ))),
    }
}

/// Parse ERROR-CODE attribute.
fn parse_error_code(attrs: &[u8]) -> (u16, String) {
    let mut pos = 0;

    while pos + 4 <= attrs.len() {
        let attr_type = u16::from_be_bytes([attrs[pos], attrs[pos + 1]]);
        let attr_len = u16::from_be_bytes([attrs[pos + 2], attrs[pos + 3]]) as usize;

        if attr_type == AttributeType::ErrorCode as u16 {
            if attrs.len() < pos + 4 + attr_len || attr_len < 4 {
                break;
            }
            let value = &attrs[pos + 4..pos + 4 + attr_len];
            let class = u16::from(value[2] & 0x07);
            let number = u16::from(value[3]);
            let code = class * 100 + number;
            let reason = String::from_utf8_lossy(&value[4..]).to_string();
            return (code, reason);
        }

        let padded_len = (attr_len + 3) & !3;
        pos += 4 + padded_len;
    }

    (0, "Unknown error".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_binding_request() {
        let transaction_id = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        let msg = build_binding_request(&transaction_id);

        // Check message type
        assert_eq!(msg[0], 0x00);
        assert_eq!(msg[1], 0x01);

        // Check message length
        assert_eq!(msg[2], 0x00);
        assert_eq!(msg[3], 0x00);

        // Check magic cookie
        assert_eq!(&msg[4..8], &MAGIC_COOKIE.to_be_bytes());

        // Check transaction ID
        assert_eq!(&msg[8..20], &transaction_id);
    }

    #[test]
    fn test_parse_binding_response_ipv4() {
        let transaction_id = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];

        // Build a valid STUN response with XOR-MAPPED-ADDRESS
        // Response for 192.0.2.1:12345
        let port: u16 = 12345;
        let ip = [192, 0, 2, 1];

        // XOR the port with upper 16 bits of magic cookie
        let x_port = port ^ ((MAGIC_COOKIE >> 16) as u16);

        // XOR the IP with magic cookie bytes
        let magic_bytes = MAGIC_COOKIE.to_be_bytes();
        let x_ip = [
            ip[0] ^ magic_bytes[0],
            ip[1] ^ magic_bytes[1],
            ip[2] ^ magic_bytes[2],
            ip[3] ^ magic_bytes[3],
        ];

        let mut response = vec![
            0x01, 0x01, // Binding Response
            0x00, 0x0c, // Message length: 12 bytes
        ];
        response.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
        response.extend_from_slice(&transaction_id);

        // XOR-MAPPED-ADDRESS attribute
        response.extend_from_slice(&[
            0x00, 0x20, // Attribute type
            0x00, 0x08, // Attribute length
            0x00, 0x01, // Reserved + Family (IPv4)
        ]);
        response.extend_from_slice(&x_port.to_be_bytes());
        response.extend_from_slice(&x_ip);

        let result = parse_binding_response(&response, &transaction_id).unwrap();

        assert_eq!(
            result,
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 1), 12345,))
        );
    }

    #[test]
    fn test_parse_binding_response_mapped_address() {
        let transaction_id = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];

        // Build response with MAPPED-ADDRESS (non-XOR)
        let mut response = vec![
            0x01, 0x01, // Binding Response
            0x00, 0x0c, // Message length: 12 bytes
        ];
        response.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
        response.extend_from_slice(&transaction_id);

        // MAPPED-ADDRESS attribute
        response.extend_from_slice(&[
            0x00, 0x01, // Attribute type
            0x00, 0x08, // Attribute length
            0x00, 0x01, // Reserved + Family (IPv4)
            0x30, 0x39, // Port: 12345
            192, 0, 2, 1, // IP address
        ]);

        let result = parse_binding_response(&response, &transaction_id).unwrap();

        assert_eq!(
            result,
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 1), 12345,))
        );
    }

    #[test]
    fn test_parse_binding_response_transaction_id_mismatch() {
        let transaction_id = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        let wrong_id = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

        let mut response = vec![
            0x01, 0x01, // Binding Response
            0x00, 0x00, // Message length: 0 bytes
        ];
        response.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
        response.extend_from_slice(&wrong_id);

        let result = parse_binding_response(&response, &transaction_id);
        assert!(matches!(result, Err(StunError::TransactionIdMismatch)));
    }

    #[test]
    fn test_parse_binding_response_too_short() {
        let transaction_id = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        let response = vec![0x01, 0x01]; // Too short

        let result = parse_binding_response(&response, &transaction_id);
        assert!(matches!(result, Err(StunError::InvalidMessage(_))));
    }

    #[test]
    fn test_message_type_from_u16() {
        assert_eq!(
            MessageType::from_u16(0x0001),
            Some(MessageType::BindingRequest)
        );
        assert_eq!(
            MessageType::from_u16(0x0101),
            Some(MessageType::BindingResponse)
        );
        assert_eq!(
            MessageType::from_u16(0x0111),
            Some(MessageType::BindingErrorResponse)
        );
        assert_eq!(MessageType::from_u16(0x9999), None);
    }

    #[test]
    fn test_attribute_type_from_u16() {
        assert_eq!(
            AttributeType::from_u16(0x0001),
            Some(AttributeType::MappedAddress)
        );
        assert_eq!(
            AttributeType::from_u16(0x0020),
            Some(AttributeType::XorMappedAddress)
        );
        assert_eq!(
            AttributeType::from_u16(0x0009),
            Some(AttributeType::ErrorCode)
        );
        assert_eq!(
            AttributeType::from_u16(0x8022),
            Some(AttributeType::Software)
        );
        assert_eq!(AttributeType::from_u16(0x9999), None);
    }

    #[test]
    fn test_stun_client_new() {
        let client = StunClient::new();
        // Just verify it can be created
        let _ = format!("{client:?}");
    }

    #[test]
    fn test_generate_transaction_id_is_random() {
        let id1 = generate_transaction_id();
        let id2 = generate_transaction_id();

        // Transaction IDs should be different (extremely high probability)
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_parse_error_response() {
        let transaction_id = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];

        // Build error response (401 Unauthorized)
        let mut response = vec![
            0x01, 0x11, // Binding Error Response
            0x00, 0x14, // Message length: 20 bytes
        ];
        response.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
        response.extend_from_slice(&transaction_id);

        // ERROR-CODE attribute
        response.extend_from_slice(&[
            0x00, 0x09, // Attribute type
            0x00, 0x10, // Attribute length: 16 bytes
            0x00, 0x00, // Reserved
            0x04, // Class: 4
            0x01, // Number: 1 (401)
        ]);
        response.extend_from_slice(b"Unauthorized"); // 12 bytes

        let result = parse_binding_response(&response, &transaction_id);

        match result {
            Err(StunError::ErrorResponse { code, reason }) => {
                assert_eq!(code, 401);
                assert_eq!(reason, "Unauthorized");
            }
            _ => panic!("Expected ErrorResponse, got {result:?}"),
        }
    }
}
