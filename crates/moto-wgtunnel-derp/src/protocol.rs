//! DERP protocol frame encoding and decoding.
//!
//! DERP (Designated Encrypted Relay for Packets) uses a simple binary frame format
//! over WebSocket connections. Each frame consists of:
//! - 1 byte: Frame type
//! - 4 bytes: Big-endian payload length
//! - N bytes: Payload (varies by frame type)
//!
//! # Protocol Constants
//!
//! - Magic: `DERP🔑` (8 bytes: `0x44 45 52 50 f0 9f 94 91`)
//! - Max packet size: 64KB
//! - Key length: 32 bytes (curve25519)
//! - Nonce length: 24 bytes
//!
//! # Frame Types
//!
//! See [`FrameType`] for all supported frame types and their payloads.

use bytes::{Buf, BufMut, Bytes, BytesMut};
use moto_wgtunnel_types::WgPublicKey;

/// Frame header length: 1 byte type + 4 bytes length.
pub const FRAME_HEADER_LEN: usize = 5;

/// Maximum packet size (64KB).
pub const MAX_PACKET_SIZE: usize = 64 << 10;

/// Key length for curve25519 public keys.
pub const KEY_LEN: usize = 32;

/// Nonce length for `NaCl` box encryption.
pub const NONCE_LEN: usize = 24;

/// Maximum client/server info JSON length.
pub const MAX_INFO_LEN: usize = 1 << 20;

/// DERP magic bytes: "DERP🔑" (8 bytes).
pub const MAGIC: [u8; 8] = [0x44, 0x45, 0x52, 0x50, 0xf0, 0x9f, 0x94, 0x91];

/// Error type for protocol operations.
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    /// Frame type is unknown or unsupported.
    #[error("unknown frame type: 0x{0:02x}")]
    UnknownFrameType(u8),

    /// Frame payload is too large.
    #[error("payload too large: {0} bytes (max {MAX_PACKET_SIZE})")]
    PayloadTooLarge(usize),

    /// Frame payload is too small for the frame type.
    #[error("payload too small: expected at least {expected} bytes, got {actual}")]
    PayloadTooSmall {
        /// Minimum expected payload size.
        expected: usize,
        /// Actual payload size received.
        actual: usize,
    },

    /// Invalid magic bytes in server key frame.
    #[error("invalid magic bytes")]
    InvalidMagic,

    /// Buffer is too small to read a complete frame.
    #[error("incomplete frame: need {needed} more bytes")]
    IncompleteFrame {
        /// Number of additional bytes needed.
        needed: usize,
    },

    /// Invalid key data.
    #[error("invalid key: {0}")]
    InvalidKey(#[from] moto_wgtunnel_types::KeyError),
}

/// DERP frame types.
///
/// Each frame type defines the structure of its payload. Frames are used for:
/// - Connection setup (`ServerKey`, `ClientInfo`, `ServerInfo`)
/// - Packet relay (`SendPacket`, `RecvPacket`)
/// - Connection management (`KeepAlive`, `Ping`, `Pong`, `Health`)
/// - Peer discovery (`PeerGone`, `PeerPresent`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameType {
    /// Server sends its public key after connection.
    /// Payload: 8B magic + 32B public key + optional future bytes.
    ServerKey = 0x01,

    /// Client sends its public key and encrypted info.
    /// Payload: 32B public key + 24B nonce + encrypted JSON.
    ClientInfo = 0x02,

    /// Server sends encrypted info to client.
    /// Payload: 24B nonce + encrypted JSON.
    ServerInfo = 0x03,

    /// Client sends a packet to another peer.
    /// Payload: 32B destination public key + packet bytes.
    SendPacket = 0x04,

    /// Server relays a packet from another peer.
    /// Payload: 32B source public key + packet bytes.
    RecvPacket = 0x05,

    /// Keep-alive frame (no payload).
    KeepAlive = 0x06,

    /// Client indicates if this is its preferred/home DERP server.
    /// Payload: 1 byte (0x01 = preferred, 0x00 = not preferred).
    NotePreferred = 0x07,

    /// Server notifies that a peer has disconnected.
    /// Payload: 32B public key + 1 byte reason code.
    PeerGone = 0x08,

    /// Server notifies that a peer is present.
    /// Payload: 32B public key + optional endpoint info.
    PeerPresent = 0x09,

    /// Server-to-server forwarded packet.
    /// Payload: 32B source key + 32B dest key + packet bytes.
    ForwardPacket = 0x0a,

    /// Request to watch connection events (mesh).
    WatchConns = 0x10,

    /// Request to close a peer connection.
    /// Payload: 32B public key of peer to close.
    ClosePeer = 0x11,

    /// Ping frame for latency measurement.
    /// Payload: 8 bytes (echoed in pong).
    Ping = 0x12,

    /// Pong frame (response to ping).
    /// Payload: 8 bytes (echoed from ping).
    Pong = 0x13,

    /// Health status frame.
    /// Payload: UTF-8 error message (empty = healthy).
    Health = 0x14,

    /// Server is restarting soon.
    /// Payload: 2 big-endian u32s (reconnect delay ms, try for ms).
    Restarting = 0x15,
}

impl FrameType {
    /// Parse a frame type from a byte.
    ///
    /// # Errors
    /// Returns error if the byte doesn't correspond to a known frame type.
    pub const fn from_byte(b: u8) -> Result<Self, ProtocolError> {
        match b {
            0x01 => Ok(Self::ServerKey),
            0x02 => Ok(Self::ClientInfo),
            0x03 => Ok(Self::ServerInfo),
            0x04 => Ok(Self::SendPacket),
            0x05 => Ok(Self::RecvPacket),
            0x06 => Ok(Self::KeepAlive),
            0x07 => Ok(Self::NotePreferred),
            0x08 => Ok(Self::PeerGone),
            0x09 => Ok(Self::PeerPresent),
            0x0a => Ok(Self::ForwardPacket),
            0x10 => Ok(Self::WatchConns),
            0x11 => Ok(Self::ClosePeer),
            0x12 => Ok(Self::Ping),
            0x13 => Ok(Self::Pong),
            0x14 => Ok(Self::Health),
            0x15 => Ok(Self::Restarting),
            _ => Err(ProtocolError::UnknownFrameType(b)),
        }
    }

    /// Get the byte value for this frame type.
    #[must_use]
    pub const fn as_byte(self) -> u8 {
        self as u8
    }
}

/// A decoded DERP frame.
///
/// Frames are the basic unit of communication in the DERP protocol.
/// Each frame has a type and a type-specific payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    /// Server's public key (sent after connection).
    ServerKey {
        /// The server's public key.
        key: WgPublicKey,
    },

    /// Client info (sent during handshake).
    ClientInfo {
        /// Client's public key.
        key: WgPublicKey,
        /// Nonce for the encrypted info.
        nonce: [u8; NONCE_LEN],
        /// Encrypted JSON info.
        encrypted_info: Bytes,
    },

    /// Server info (sent during handshake).
    ServerInfo {
        /// Nonce for the encrypted info.
        nonce: [u8; NONCE_LEN],
        /// Encrypted JSON info.
        encrypted_info: Bytes,
    },

    /// Send a packet to a peer.
    SendPacket {
        /// Destination peer's public key.
        dst: WgPublicKey,
        /// Packet data.
        data: Bytes,
    },

    /// Receive a packet from a peer.
    RecvPacket {
        /// Source peer's public key.
        src: WgPublicKey,
        /// Packet data.
        data: Bytes,
    },

    /// Keep-alive (no payload).
    KeepAlive,

    /// Note whether this is the preferred DERP server.
    NotePreferred {
        /// True if this is the client's preferred/home server.
        preferred: bool,
    },

    /// A peer has disconnected.
    PeerGone {
        /// The peer's public key.
        key: WgPublicKey,
        /// Reason code for disconnect.
        reason: PeerGoneReason,
    },

    /// A peer is present.
    PeerPresent {
        /// The peer's public key.
        key: WgPublicKey,
    },

    /// Forwarded packet (mesh mode).
    ForwardPacket {
        /// Source peer's public key.
        src: WgPublicKey,
        /// Destination peer's public key.
        dst: WgPublicKey,
        /// Packet data.
        data: Bytes,
    },

    /// Watch connection events (mesh mode).
    WatchConns,

    /// Request to close a peer.
    ClosePeer {
        /// The peer's public key.
        key: WgPublicKey,
    },

    /// Ping for latency measurement.
    Ping {
        /// 8-byte ping data (echoed in pong).
        data: [u8; 8],
    },

    /// Pong response to ping.
    Pong {
        /// 8-byte pong data (echoed from ping).
        data: [u8; 8],
    },

    /// Health status.
    Health {
        /// Error message (empty means healthy).
        message: String,
    },

    /// Server is restarting.
    Restarting {
        /// Milliseconds before reconnecting.
        reconnect_in_ms: u32,
        /// Milliseconds to keep trying.
        try_for_ms: u32,
    },
}

/// Reason a peer disconnected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PeerGoneReason {
    /// Unknown reason.
    Unknown = 0x00,
    /// Peer disconnected gracefully.
    Disconnected = 0x01,
    /// Peer was not found.
    NotHere = 0x02,
}

impl PeerGoneReason {
    /// Parse from a byte.
    #[must_use]
    pub const fn from_byte(b: u8) -> Self {
        match b {
            0x01 => Self::Disconnected,
            0x02 => Self::NotHere,
            _ => Self::Unknown,
        }
    }

    /// Get the byte value.
    #[must_use]
    pub const fn as_byte(self) -> u8 {
        self as u8
    }
}

impl Frame {
    /// Get the frame type for this frame.
    #[must_use]
    pub const fn frame_type(&self) -> FrameType {
        match self {
            Self::ServerKey { .. } => FrameType::ServerKey,
            Self::ClientInfo { .. } => FrameType::ClientInfo,
            Self::ServerInfo { .. } => FrameType::ServerInfo,
            Self::SendPacket { .. } => FrameType::SendPacket,
            Self::RecvPacket { .. } => FrameType::RecvPacket,
            Self::KeepAlive => FrameType::KeepAlive,
            Self::NotePreferred { .. } => FrameType::NotePreferred,
            Self::PeerGone { .. } => FrameType::PeerGone,
            Self::PeerPresent { .. } => FrameType::PeerPresent,
            Self::ForwardPacket { .. } => FrameType::ForwardPacket,
            Self::WatchConns => FrameType::WatchConns,
            Self::ClosePeer { .. } => FrameType::ClosePeer,
            Self::Ping { .. } => FrameType::Ping,
            Self::Pong { .. } => FrameType::Pong,
            Self::Health { .. } => FrameType::Health,
            Self::Restarting { .. } => FrameType::Restarting,
        }
    }

    /// Encode this frame into a buffer.
    ///
    /// Writes the frame header (type + length) followed by the payload.
    #[allow(clippy::too_many_lines, clippy::cast_possible_truncation)]
    pub fn encode(&self, buf: &mut BytesMut) {
        match self {
            Self::ServerKey { key } => {
                let payload_len = MAGIC.len() + KEY_LEN;
                buf.put_u8(FrameType::ServerKey.as_byte());
                buf.put_u32(payload_len as u32);
                buf.put_slice(&MAGIC);
                buf.put_slice(key.as_bytes());
            }
            Self::ClientInfo {
                key,
                nonce,
                encrypted_info,
            } => {
                let payload_len = KEY_LEN + NONCE_LEN + encrypted_info.len();
                buf.put_u8(FrameType::ClientInfo.as_byte());
                buf.put_u32(payload_len as u32);
                buf.put_slice(key.as_bytes());
                buf.put_slice(nonce);
                buf.put_slice(encrypted_info);
            }
            Self::ServerInfo {
                nonce,
                encrypted_info,
            } => {
                let payload_len = NONCE_LEN + encrypted_info.len();
                buf.put_u8(FrameType::ServerInfo.as_byte());
                buf.put_u32(payload_len as u32);
                buf.put_slice(nonce);
                buf.put_slice(encrypted_info);
            }
            Self::SendPacket { dst, data } => {
                let payload_len = KEY_LEN + data.len();
                buf.put_u8(FrameType::SendPacket.as_byte());
                buf.put_u32(payload_len as u32);
                buf.put_slice(dst.as_bytes());
                buf.put_slice(data);
            }
            Self::RecvPacket { src, data } => {
                let payload_len = KEY_LEN + data.len();
                buf.put_u8(FrameType::RecvPacket.as_byte());
                buf.put_u32(payload_len as u32);
                buf.put_slice(src.as_bytes());
                buf.put_slice(data);
            }
            Self::KeepAlive => {
                buf.put_u8(FrameType::KeepAlive.as_byte());
                buf.put_u32(0);
            }
            Self::NotePreferred { preferred } => {
                buf.put_u8(FrameType::NotePreferred.as_byte());
                buf.put_u32(1);
                buf.put_u8(u8::from(*preferred));
            }
            Self::PeerGone { key, reason } => {
                let payload_len = KEY_LEN + 1;
                buf.put_u8(FrameType::PeerGone.as_byte());
                buf.put_u32(payload_len as u32);
                buf.put_slice(key.as_bytes());
                buf.put_u8(reason.as_byte());
            }
            Self::PeerPresent { key } => {
                buf.put_u8(FrameType::PeerPresent.as_byte());
                buf.put_u32(KEY_LEN as u32);
                buf.put_slice(key.as_bytes());
            }
            Self::ForwardPacket { src, dst, data } => {
                let payload_len = KEY_LEN + KEY_LEN + data.len();
                buf.put_u8(FrameType::ForwardPacket.as_byte());
                buf.put_u32(payload_len as u32);
                buf.put_slice(src.as_bytes());
                buf.put_slice(dst.as_bytes());
                buf.put_slice(data);
            }
            Self::WatchConns => {
                buf.put_u8(FrameType::WatchConns.as_byte());
                buf.put_u32(0);
            }
            Self::ClosePeer { key } => {
                buf.put_u8(FrameType::ClosePeer.as_byte());
                buf.put_u32(KEY_LEN as u32);
                buf.put_slice(key.as_bytes());
            }
            Self::Ping { data } => {
                buf.put_u8(FrameType::Ping.as_byte());
                buf.put_u32(8);
                buf.put_slice(data);
            }
            Self::Pong { data } => {
                buf.put_u8(FrameType::Pong.as_byte());
                buf.put_u32(8);
                buf.put_slice(data);
            }
            Self::Health { message } => {
                let payload_len = message.len();
                buf.put_u8(FrameType::Health.as_byte());
                buf.put_u32(payload_len as u32);
                buf.put_slice(message.as_bytes());
            }
            Self::Restarting {
                reconnect_in_ms,
                try_for_ms,
            } => {
                buf.put_u8(FrameType::Restarting.as_byte());
                buf.put_u32(8);
                buf.put_u32(*reconnect_in_ms);
                buf.put_u32(*try_for_ms);
            }
        }
    }

    /// Encode this frame into a new `Bytes`.
    #[must_use]
    pub fn encode_to_bytes(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(FRAME_HEADER_LEN + MAX_PACKET_SIZE);
        self.encode(&mut buf);
        buf.freeze()
    }
}

/// Decode a frame from a buffer.
///
/// Returns the decoded frame and advances the buffer past the consumed bytes.
///
/// # Errors
/// Returns error if:
/// - Buffer is too small for a complete frame
/// - Frame type is unknown
/// - Payload is invalid for the frame type
#[allow(clippy::too_many_lines)]
pub fn decode_frame(buf: &mut impl Buf) -> Result<Frame, ProtocolError> {
    if buf.remaining() < FRAME_HEADER_LEN {
        return Err(ProtocolError::IncompleteFrame {
            needed: FRAME_HEADER_LEN - buf.remaining(),
        });
    }

    let frame_type_byte = buf.get_u8();
    let payload_len = buf.get_u32() as usize;

    if payload_len > MAX_PACKET_SIZE {
        return Err(ProtocolError::PayloadTooLarge(payload_len));
    }

    if buf.remaining() < payload_len {
        return Err(ProtocolError::IncompleteFrame {
            needed: payload_len - buf.remaining(),
        });
    }

    let frame_type = FrameType::from_byte(frame_type_byte)?;

    match frame_type {
        FrameType::ServerKey => {
            if payload_len < MAGIC.len() + KEY_LEN {
                return Err(ProtocolError::PayloadTooSmall {
                    expected: MAGIC.len() + KEY_LEN,
                    actual: payload_len,
                });
            }
            let mut magic_buf = [0u8; 8];
            buf.copy_to_slice(&mut magic_buf);
            if magic_buf != MAGIC {
                return Err(ProtocolError::InvalidMagic);
            }
            let mut key_buf = [0u8; KEY_LEN];
            buf.copy_to_slice(&mut key_buf);
            let key = WgPublicKey::from_bytes(&key_buf)?;
            // Skip any extra bytes (future use)
            let extra = payload_len - MAGIC.len() - KEY_LEN;
            buf.advance(extra);
            Ok(Frame::ServerKey { key })
        }
        FrameType::ClientInfo => {
            if payload_len < KEY_LEN + NONCE_LEN {
                return Err(ProtocolError::PayloadTooSmall {
                    expected: KEY_LEN + NONCE_LEN,
                    actual: payload_len,
                });
            }
            let mut key_buf = [0u8; KEY_LEN];
            buf.copy_to_slice(&mut key_buf);
            let key = WgPublicKey::from_bytes(&key_buf)?;
            let mut nonce = [0u8; NONCE_LEN];
            buf.copy_to_slice(&mut nonce);
            let info_len = payload_len - KEY_LEN - NONCE_LEN;
            let encrypted_info = buf.copy_to_bytes(info_len);
            Ok(Frame::ClientInfo {
                key,
                nonce,
                encrypted_info,
            })
        }
        FrameType::ServerInfo => {
            if payload_len < NONCE_LEN {
                return Err(ProtocolError::PayloadTooSmall {
                    expected: NONCE_LEN,
                    actual: payload_len,
                });
            }
            let mut nonce = [0u8; NONCE_LEN];
            buf.copy_to_slice(&mut nonce);
            let info_len = payload_len - NONCE_LEN;
            let encrypted_info = buf.copy_to_bytes(info_len);
            Ok(Frame::ServerInfo {
                nonce,
                encrypted_info,
            })
        }
        FrameType::SendPacket => {
            if payload_len < KEY_LEN {
                return Err(ProtocolError::PayloadTooSmall {
                    expected: KEY_LEN,
                    actual: payload_len,
                });
            }
            let mut key_buf = [0u8; KEY_LEN];
            buf.copy_to_slice(&mut key_buf);
            let dst = WgPublicKey::from_bytes(&key_buf)?;
            let data_len = payload_len - KEY_LEN;
            let data = buf.copy_to_bytes(data_len);
            Ok(Frame::SendPacket { dst, data })
        }
        FrameType::RecvPacket => {
            if payload_len < KEY_LEN {
                return Err(ProtocolError::PayloadTooSmall {
                    expected: KEY_LEN,
                    actual: payload_len,
                });
            }
            let mut key_buf = [0u8; KEY_LEN];
            buf.copy_to_slice(&mut key_buf);
            let src = WgPublicKey::from_bytes(&key_buf)?;
            let data_len = payload_len - KEY_LEN;
            let data = buf.copy_to_bytes(data_len);
            Ok(Frame::RecvPacket { src, data })
        }
        FrameType::KeepAlive => {
            buf.advance(payload_len);
            Ok(Frame::KeepAlive)
        }
        FrameType::NotePreferred => {
            if payload_len < 1 {
                return Err(ProtocolError::PayloadTooSmall {
                    expected: 1,
                    actual: payload_len,
                });
            }
            let preferred = buf.get_u8() != 0;
            buf.advance(payload_len - 1);
            Ok(Frame::NotePreferred { preferred })
        }
        FrameType::PeerGone => {
            if payload_len < KEY_LEN + 1 {
                return Err(ProtocolError::PayloadTooSmall {
                    expected: KEY_LEN + 1,
                    actual: payload_len,
                });
            }
            let mut key_buf = [0u8; KEY_LEN];
            buf.copy_to_slice(&mut key_buf);
            let key = WgPublicKey::from_bytes(&key_buf)?;
            let reason = PeerGoneReason::from_byte(buf.get_u8());
            buf.advance(payload_len - KEY_LEN - 1);
            Ok(Frame::PeerGone { key, reason })
        }
        FrameType::PeerPresent => {
            if payload_len < KEY_LEN {
                return Err(ProtocolError::PayloadTooSmall {
                    expected: KEY_LEN,
                    actual: payload_len,
                });
            }
            let mut key_buf = [0u8; KEY_LEN];
            buf.copy_to_slice(&mut key_buf);
            let key = WgPublicKey::from_bytes(&key_buf)?;
            // Skip optional endpoint info
            buf.advance(payload_len - KEY_LEN);
            Ok(Frame::PeerPresent { key })
        }
        FrameType::ForwardPacket => {
            if payload_len < KEY_LEN + KEY_LEN {
                return Err(ProtocolError::PayloadTooSmall {
                    expected: KEY_LEN + KEY_LEN,
                    actual: payload_len,
                });
            }
            let mut src_buf = [0u8; KEY_LEN];
            buf.copy_to_slice(&mut src_buf);
            let src = WgPublicKey::from_bytes(&src_buf)?;
            let mut dst_buf = [0u8; KEY_LEN];
            buf.copy_to_slice(&mut dst_buf);
            let dst = WgPublicKey::from_bytes(&dst_buf)?;
            let data_len = payload_len - KEY_LEN - KEY_LEN;
            let data = buf.copy_to_bytes(data_len);
            Ok(Frame::ForwardPacket { src, dst, data })
        }
        FrameType::WatchConns => {
            buf.advance(payload_len);
            Ok(Frame::WatchConns)
        }
        FrameType::ClosePeer => {
            if payload_len < KEY_LEN {
                return Err(ProtocolError::PayloadTooSmall {
                    expected: KEY_LEN,
                    actual: payload_len,
                });
            }
            let mut key_buf = [0u8; KEY_LEN];
            buf.copy_to_slice(&mut key_buf);
            let key = WgPublicKey::from_bytes(&key_buf)?;
            buf.advance(payload_len - KEY_LEN);
            Ok(Frame::ClosePeer { key })
        }
        FrameType::Ping => {
            if payload_len < 8 {
                return Err(ProtocolError::PayloadTooSmall {
                    expected: 8,
                    actual: payload_len,
                });
            }
            let mut data = [0u8; 8];
            buf.copy_to_slice(&mut data);
            buf.advance(payload_len - 8);
            Ok(Frame::Ping { data })
        }
        FrameType::Pong => {
            if payload_len < 8 {
                return Err(ProtocolError::PayloadTooSmall {
                    expected: 8,
                    actual: payload_len,
                });
            }
            let mut data = [0u8; 8];
            buf.copy_to_slice(&mut data);
            buf.advance(payload_len - 8);
            Ok(Frame::Pong { data })
        }
        FrameType::Health => {
            let mut msg_buf = vec![0u8; payload_len];
            buf.copy_to_slice(&mut msg_buf);
            let message = String::from_utf8_lossy(&msg_buf).into_owned();
            Ok(Frame::Health { message })
        }
        FrameType::Restarting => {
            if payload_len < 8 {
                return Err(ProtocolError::PayloadTooSmall {
                    expected: 8,
                    actual: payload_len,
                });
            }
            let reconnect_in_ms = buf.get_u32();
            let try_for_ms = buf.get_u32();
            buf.advance(payload_len - 8);
            Ok(Frame::Restarting {
                reconnect_in_ms,
                try_for_ms,
            })
        }
    }
}

/// Check if a buffer contains a complete frame.
///
/// Returns `Ok(total_frame_size)` if complete, or `Err(needed_bytes)` if incomplete.
///
/// # Errors
/// Returns `Err(needed_bytes)` if the buffer doesn't contain a complete frame.
#[allow(clippy::missing_errors_doc)]
pub fn check_frame_complete(buf: &[u8]) -> Result<usize, usize> {
    if buf.len() < FRAME_HEADER_LEN {
        return Err(FRAME_HEADER_LEN - buf.len());
    }

    let payload_len = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]) as usize;
    let total_len = FRAME_HEADER_LEN + payload_len;

    if buf.len() < total_len {
        Err(total_len - buf.len())
    } else {
        Ok(total_len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use moto_wgtunnel_types::WgPrivateKey;

    fn test_key() -> WgPublicKey {
        WgPrivateKey::generate().public_key()
    }

    #[test]
    fn frame_type_roundtrip() {
        for byte in 0x00..=0xff {
            if let Ok(ft) = FrameType::from_byte(byte) {
                assert_eq!(ft.as_byte(), byte);
            }
        }
    }

    #[test]
    fn server_key_encode_decode() {
        let key = test_key();
        let frame = Frame::ServerKey { key: key.clone() };

        let encoded = frame.encode_to_bytes();
        let mut buf = encoded;
        let decoded = decode_frame(&mut buf).unwrap();

        assert_eq!(decoded, Frame::ServerKey { key });
        assert!(buf.is_empty());
    }

    #[test]
    fn send_packet_encode_decode() {
        let dst = test_key();
        let data = Bytes::from_static(b"hello world");
        let frame = Frame::SendPacket {
            dst: dst.clone(),
            data: data.clone(),
        };

        let encoded = frame.encode_to_bytes();
        let mut buf = encoded;
        let decoded = decode_frame(&mut buf).unwrap();

        assert_eq!(decoded, Frame::SendPacket { dst, data });
    }

    #[test]
    fn recv_packet_encode_decode() {
        let src = test_key();
        let data = Bytes::from_static(b"encrypted wireguard packet");
        let frame = Frame::RecvPacket {
            src: src.clone(),
            data: data.clone(),
        };

        let encoded = frame.encode_to_bytes();
        let mut buf = encoded;
        let decoded = decode_frame(&mut buf).unwrap();

        assert_eq!(decoded, Frame::RecvPacket { src, data });
    }

    #[test]
    fn keepalive_encode_decode() {
        let frame = Frame::KeepAlive;
        let encoded = frame.encode_to_bytes();
        let mut buf = encoded;
        let decoded = decode_frame(&mut buf).unwrap();

        assert_eq!(decoded, Frame::KeepAlive);
    }

    #[test]
    fn ping_pong_encode_decode() {
        let ping_data = [1, 2, 3, 4, 5, 6, 7, 8];
        let ping = Frame::Ping { data: ping_data };
        let pong = Frame::Pong { data: ping_data };

        let encoded_ping = ping.encode_to_bytes();
        let mut buf = encoded_ping;
        assert_eq!(
            decode_frame(&mut buf).unwrap(),
            Frame::Ping { data: ping_data }
        );

        let encoded_pong = pong.encode_to_bytes();
        let mut buf = encoded_pong;
        assert_eq!(
            decode_frame(&mut buf).unwrap(),
            Frame::Pong { data: ping_data }
        );
    }

    #[test]
    fn note_preferred_encode_decode() {
        for preferred in [true, false] {
            let frame = Frame::NotePreferred { preferred };
            let encoded = frame.encode_to_bytes();
            let mut buf = encoded.clone();
            let decoded = decode_frame(&mut buf).unwrap();

            assert_eq!(decoded, Frame::NotePreferred { preferred });
        }
    }

    #[test]
    fn peer_gone_encode_decode() {
        let key = test_key();
        for reason in [
            PeerGoneReason::Unknown,
            PeerGoneReason::Disconnected,
            PeerGoneReason::NotHere,
        ] {
            let frame = Frame::PeerGone {
                key: key.clone(),
                reason,
            };
            let encoded = frame.encode_to_bytes();
            let mut buf = encoded.clone();
            let decoded = decode_frame(&mut buf).unwrap();

            assert_eq!(
                decoded,
                Frame::PeerGone {
                    key: key.clone(),
                    reason
                }
            );
        }
    }

    #[test]
    fn health_encode_decode() {
        let frame = Frame::Health {
            message: "all good".to_string(),
        };
        let encoded = frame.encode_to_bytes();
        let mut buf = encoded;
        let decoded = decode_frame(&mut buf).unwrap();

        assert_eq!(
            decoded,
            Frame::Health {
                message: "all good".to_string()
            }
        );
    }

    #[test]
    fn restarting_encode_decode() {
        let frame = Frame::Restarting {
            reconnect_in_ms: 1000,
            try_for_ms: 5000,
        };
        let encoded = frame.encode_to_bytes();
        let mut buf = encoded;
        let decoded = decode_frame(&mut buf).unwrap();

        assert_eq!(
            decoded,
            Frame::Restarting {
                reconnect_in_ms: 1000,
                try_for_ms: 5000
            }
        );
    }

    #[test]
    fn forward_packet_encode_decode() {
        let src = test_key();
        let dst = test_key();
        let data = Bytes::from_static(b"forwarded data");
        let frame = Frame::ForwardPacket {
            src: src.clone(),
            dst: dst.clone(),
            data: data.clone(),
        };

        let encoded = frame.encode_to_bytes();
        let mut buf = encoded;
        let decoded = decode_frame(&mut buf).unwrap();

        assert_eq!(decoded, Frame::ForwardPacket { src, dst, data });
    }

    #[test]
    fn client_info_encode_decode() {
        let key = test_key();
        let nonce = [1u8; NONCE_LEN];
        let encrypted_info = Bytes::from_static(b"encrypted json");
        let frame = Frame::ClientInfo {
            key: key.clone(),
            nonce,
            encrypted_info: encrypted_info.clone(),
        };

        let encoded = frame.encode_to_bytes();
        let mut buf = encoded;
        let decoded = decode_frame(&mut buf).unwrap();

        assert_eq!(
            decoded,
            Frame::ClientInfo {
                key,
                nonce,
                encrypted_info
            }
        );
    }

    #[test]
    fn server_info_encode_decode() {
        let nonce = [2u8; NONCE_LEN];
        let encrypted_info = Bytes::from_static(b"encrypted server info");
        let frame = Frame::ServerInfo {
            nonce,
            encrypted_info: encrypted_info.clone(),
        };

        let encoded = frame.encode_to_bytes();
        let mut buf = encoded;
        let decoded = decode_frame(&mut buf).unwrap();

        assert_eq!(
            decoded,
            Frame::ServerInfo {
                nonce,
                encrypted_info
            }
        );
    }

    #[test]
    fn incomplete_frame_error() {
        let mut buf = Bytes::from_static(&[0x01, 0x00, 0x00]); // Only 3 bytes, need 5 for header
        let result = decode_frame(&mut buf);
        assert!(matches!(result, Err(ProtocolError::IncompleteFrame { .. })));
    }

    #[test]
    fn unknown_frame_type_error() {
        let mut buf = Bytes::from_static(&[0xff, 0x00, 0x00, 0x00, 0x00]);
        let result = decode_frame(&mut buf);
        assert!(matches!(result, Err(ProtocolError::UnknownFrameType(0xff))));
    }

    #[test]
    fn payload_too_large_error() {
        let mut buf = Bytes::from_static(&[0x06, 0xff, 0xff, 0xff, 0xff]);
        let result = decode_frame(&mut buf);
        assert!(matches!(result, Err(ProtocolError::PayloadTooLarge(_))));
    }

    #[test]
    fn invalid_magic_error() {
        let mut data = vec![FrameType::ServerKey.as_byte()];
        data.extend_from_slice(&(40u32).to_be_bytes()); // 8 magic + 32 key
        data.extend_from_slice(&[0x00; 8]); // Wrong magic
        data.extend_from_slice(&[0x00; 32]); // Key

        let mut buf = Bytes::from(data);
        let result = decode_frame(&mut buf);
        assert!(matches!(result, Err(ProtocolError::InvalidMagic)));
    }

    #[test]
    fn check_frame_complete_works() {
        // Too small for header
        assert_eq!(check_frame_complete(&[0x01, 0x00]), Err(3));

        // Header present but payload incomplete
        assert_eq!(
            check_frame_complete(&[0x06, 0x00, 0x00, 0x00, 0x05, 0x00]),
            Err(4)
        );

        // Complete frame
        assert_eq!(check_frame_complete(&[0x06, 0x00, 0x00, 0x00, 0x00]), Ok(5));

        // Complete frame with payload
        assert_eq!(
            check_frame_complete(&[0x07, 0x00, 0x00, 0x00, 0x01, 0x01]),
            Ok(6)
        );
    }

    #[test]
    fn frame_type_returns_correct_value() {
        assert_eq!(Frame::KeepAlive.frame_type(), FrameType::KeepAlive);
        assert_eq!(Frame::Ping { data: [0; 8] }.frame_type(), FrameType::Ping);
        assert_eq!(
            Frame::ServerKey { key: test_key() }.frame_type(),
            FrameType::ServerKey
        );
    }
}
