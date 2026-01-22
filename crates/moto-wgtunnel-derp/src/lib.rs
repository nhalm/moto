//! DERP (Designated Encrypted Relay for Packets) protocol implementation.
//!
//! DERP provides relay when direct P2P `WireGuard` connections fail due to NAT.
//! Traffic is already `WireGuard`-encrypted before reaching DERP, so DERP only
//! forwards opaque encrypted packets and never sees plaintext.
//!
//! # Architecture
//!
//! ```text
//! ┌──────────┐                    ┌─────────────┐                    ┌──────────┐
//! │  Client  │ ──WG encrypted──▶ │ DERP Server │ ──WG encrypted──▶ │  Peer    │
//! │          │ ◀──WG encrypted── │   (relay)   │ ◀──WG encrypted── │          │
//! └──────────┘                    └─────────────┘                    └──────────┘
//! ```
//!
//! # Protocol
//!
//! DERP uses a simple binary frame format over WebSocket:
//! - 1 byte: Frame type
//! - 4 bytes: Big-endian payload length
//! - N bytes: Payload
//!
//! See [`protocol`] module for frame encoding/decoding.
//!
//! # Example
//!
//! ```
//! use moto_wgtunnel_derp::protocol::{Frame, decode_frame, MAGIC};
//! use moto_wgtunnel_types::WgPrivateKey;
//! use bytes::BytesMut;
//!
//! // Create a keepalive frame
//! let frame = Frame::KeepAlive;
//! let mut buf = BytesMut::new();
//! frame.encode(&mut buf);
//!
//! // Decode the frame
//! let mut read_buf = buf.freeze();
//! let decoded = decode_frame(&mut read_buf).unwrap();
//! assert!(matches!(decoded, Frame::KeepAlive));
//! ```
//!
//! ```
//! use moto_wgtunnel_derp::protocol::{Frame, decode_frame};
//! use moto_wgtunnel_types::WgPrivateKey;
//! use bytes::Bytes;
//!
//! // Create a send packet frame
//! let key = WgPrivateKey::generate().public_key();
//! let frame = Frame::SendPacket {
//!     dst: key.clone(),
//!     data: Bytes::from_static(b"encrypted wireguard packet"),
//! };
//!
//! // Encode and decode roundtrip
//! let encoded = frame.encode_to_bytes();
//! let mut buf = encoded;
//! let decoded = decode_frame(&mut buf).unwrap();
//!
//! match decoded {
//!     Frame::SendPacket { dst, data } => {
//!         assert_eq!(dst, key);
//!         assert_eq!(data.as_ref(), b"encrypted wireguard packet");
//!     }
//!     _ => panic!("wrong frame type"),
//! }
//! ```

pub mod protocol;

pub use protocol::{
    check_frame_complete, decode_frame, Frame, FrameType, PeerGoneReason, ProtocolError,
    FRAME_HEADER_LEN, KEY_LEN, MAGIC, MAX_INFO_LEN, MAX_PACKET_SIZE, NONCE_LEN,
};
