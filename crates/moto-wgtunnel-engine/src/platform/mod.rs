//! Platform-specific TUN abstractions.
//!
//! This module provides cross-platform TUN device abstractions for Linux and macOS.
//! The TUN device is used to route IP packets through the WireGuard tunnel.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────┐
//! │                         Platform TUN Abstraction                             │
//! │  ┌───────────────────────────────────────────────────────────────────────┐  │
//! │  │                              TunDevice                                 │  │
//! │  │  - Unified interface for TUN operations                               │  │
//! │  │  - Read/write IP packets                                              │  │
//! │  │  - Configure MTU and IP address                                       │  │
//! │  └───────────────────────────────────────────────────────────────────────┘  │
//! │                                │                                             │
//! │                ┌───────────────┴───────────────┐                            │
//! │                │                               │                            │
//! │                ▼                               ▼                            │
//! │  ┌─────────────────────────┐    ┌─────────────────────────────────┐        │
//! │  │ Linux Implementation    │    │ macOS Implementation            │        │
//! │  │ - /dev/net/tun          │    │ - utun device via ioctl         │        │
//! │  │ - IFF_TUN | IFF_NO_PI   │    │ - SYSPROTO_CONTROL              │        │
//! │  └─────────────────────────┘    └─────────────────────────────────┘        │
//! └─────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```ignore
//! use moto_wgtunnel_engine::platform::{TunDevice, TunConfig};
//!
//! // Create a TUN device
//! let config = TunConfig::new()
//!     .name("moto0")
//!     .mtu(1420);
//! let mut tun = TunDevice::create(config)?;
//!
//! // Set the IP address (requires root on some platforms)
//! tun.set_ip("fd00:moto:2::1".parse()?)?;
//!
//! // Read and write packets
//! let mut buf = [0u8; 2048];
//! let n = tun.read(&mut buf)?;
//! tun.write(&buf[..n])?;
//! ```
//!
//! # Platform Support
//!
//! | Platform | Support | Implementation |
//! |----------|---------|----------------|
//! | Linux | Full | `/dev/net/tun` with `IFF_TUN` |
//! | macOS | Full | `utun` via `SYSPROTO_CONTROL` |
//! | Windows | Not supported | N/A |
//!
//! # Virtual TUN Mode
//!
//! For userspace operation without kernel TUN devices (e.g., in containers or
//! without root), use [`VirtualTun`] which provides an in-process packet queue.
//! This is the default mode for moto-cli.

use std::io;
use std::net::Ipv6Addr;
use thiserror::Error;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "linux")]
pub use linux::PlatformTun;
#[cfg(target_os = "macos")]
pub use macos::PlatformTun;

/// Default TUN device name prefix.
pub const DEFAULT_TUN_NAME: &str = "moto";

/// Default MTU for TUN devices.
pub const DEFAULT_TUN_MTU: u16 = 1420;

/// Errors that can occur during TUN operations.
#[derive(Debug, Error)]
pub enum TunError {
    /// Failed to create TUN device.
    #[error("failed to create TUN device: {0}")]
    Create(String),

    /// Failed to configure TUN device.
    #[error("failed to configure TUN device: {0}")]
    Configure(String),

    /// Failed to read from TUN device.
    #[error("failed to read from TUN device: {0}")]
    Read(#[source] io::Error),

    /// Failed to write to TUN device.
    #[error("failed to write to TUN device: {0}")]
    Write(#[source] io::Error),

    /// TUN device not found or not available.
    #[error("TUN device not available: {0}")]
    NotAvailable(String),

    /// Permission denied.
    #[error("permission denied: {0}")]
    PermissionDenied(String),

    /// Platform not supported.
    #[error("platform not supported")]
    PlatformNotSupported,

    /// Device is closed.
    #[error("TUN device is closed")]
    Closed,

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
}

/// Configuration for creating a TUN device.
#[derive(Debug, Clone)]
pub struct TunConfig {
    /// Device name (e.g., "moto0"). If empty, a name will be auto-assigned.
    pub name: String,

    /// MTU for the device.
    pub mtu: u16,

    /// Whether to use virtual (in-process) TUN instead of kernel device.
    ///
    /// Virtual TUN doesn't require root/sudo and works in containers.
    pub virtual_tun: bool,
}

impl Default for TunConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            mtu: DEFAULT_TUN_MTU,
            virtual_tun: true, // Default to virtual for moto-cli
        }
    }
}

impl TunConfig {
    /// Create a new TUN configuration with defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the device name.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Set the MTU.
    #[must_use]
    pub fn mtu(mut self, mtu: u16) -> Self {
        self.mtu = mtu;
        self
    }

    /// Use virtual (in-process) TUN instead of kernel device.
    #[must_use]
    pub fn virtual_tun(mut self, virtual_tun: bool) -> Self {
        self.virtual_tun = virtual_tun;
        self
    }
}

/// Information about a TUN device.
#[derive(Debug, Clone)]
pub struct TunInfo {
    /// Device name.
    pub name: String,

    /// MTU.
    pub mtu: u16,

    /// Whether this is a virtual TUN.
    pub is_virtual: bool,

    /// Assigned IPv6 address, if any.
    pub ipv6_addr: Option<Ipv6Addr>,
}

/// A platform-agnostic TUN device.
///
/// This enum wraps platform-specific implementations or a virtual TUN.
pub enum TunDevice {
    /// Platform-specific kernel TUN device.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    Platform(PlatformTun),

    /// Virtual (in-process) TUN device.
    Virtual(VirtualTun),
}

impl TunDevice {
    /// Create a TUN device with the given configuration.
    ///
    /// # Errors
    ///
    /// Returns error if TUN device creation fails.
    pub fn create(config: TunConfig) -> Result<Self, TunError> {
        if config.virtual_tun {
            Ok(Self::Virtual(VirtualTun::new(config)))
        } else {
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            {
                Ok(Self::Platform(PlatformTun::create(config)?))
            }
            #[cfg(not(any(target_os = "linux", target_os = "macos")))]
            {
                Err(TunError::PlatformNotSupported)
            }
        }
    }

    /// Create a virtual TUN device.
    ///
    /// This is a convenience method for creating an in-process TUN.
    #[must_use]
    pub fn virtual_tun(config: TunConfig) -> Self {
        Self::Virtual(VirtualTun::new(config))
    }

    /// Get information about the TUN device.
    #[must_use]
    pub fn info(&self) -> TunInfo {
        match self {
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            Self::Platform(tun) => tun.info(),
            Self::Virtual(tun) => tun.info(),
        }
    }

    /// Get the device name.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            Self::Platform(tun) => tun.name(),
            Self::Virtual(tun) => tun.name(),
        }
    }

    /// Get the MTU.
    #[must_use]
    pub fn mtu(&self) -> u16 {
        match self {
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            Self::Platform(tun) => tun.mtu(),
            Self::Virtual(tun) => tun.mtu(),
        }
    }

    /// Check if this is a virtual TUN.
    #[must_use]
    pub fn is_virtual(&self) -> bool {
        matches!(self, Self::Virtual(_))
    }

    /// Set the IPv6 address for the TUN device.
    ///
    /// # Errors
    ///
    /// Returns error if setting the address fails (may require root).
    pub fn set_ipv6(&mut self, addr: Ipv6Addr) -> Result<(), TunError> {
        match self {
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            Self::Platform(tun) => tun.set_ipv6(addr),
            Self::Virtual(tun) => tun.set_ipv6(addr),
        }
    }

    /// Read a packet from the TUN device.
    ///
    /// Returns the number of bytes read.
    ///
    /// # Errors
    ///
    /// Returns error if read fails.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, TunError> {
        match self {
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            Self::Platform(tun) => tun.read(buf),
            Self::Virtual(tun) => tun.read(buf),
        }
    }

    /// Write a packet to the TUN device.
    ///
    /// Returns the number of bytes written.
    ///
    /// # Errors
    ///
    /// Returns error if write fails.
    pub fn write(&mut self, buf: &[u8]) -> Result<usize, TunError> {
        match self {
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            Self::Platform(tun) => tun.write(buf),
            Self::Virtual(tun) => tun.write(buf),
        }
    }

    /// Inject a packet into the TUN device (for virtual TUN).
    ///
    /// This is used to deliver decrypted packets from the WireGuard tunnel
    /// to the application layer.
    ///
    /// # Errors
    ///
    /// Returns error if injection fails.
    pub fn inject(&mut self, buf: &[u8]) -> Result<(), TunError> {
        match self {
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            Self::Platform(_) => {
                // For platform TUN, write directly
                self.write(buf)?;
                Ok(())
            }
            Self::Virtual(tun) => tun.inject(buf),
        }
    }

    /// Close the TUN device.
    pub fn close(&mut self) {
        match self {
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            Self::Platform(tun) => tun.close(),
            Self::Virtual(tun) => tun.close(),
        }
    }
}

impl std::fmt::Debug for TunDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            Self::Platform(tun) => f.debug_tuple("Platform").field(tun).finish(),
            Self::Virtual(tun) => f.debug_tuple("Virtual").field(tun).finish(),
        }
    }
}

/// Virtual (in-process) TUN device.
///
/// This provides a TUN-like interface without requiring kernel TUN devices.
/// Packets are queued in-process, making it suitable for:
/// - Running without root/sudo
/// - Containerized environments
/// - Testing
///
/// # Architecture
///
/// ```text
/// ┌─────────────────────────────────────────────────────────────────────────┐
/// │                           VirtualTun                                     │
/// │  ┌─────────────────────────────────────────────────────────────────┐    │
/// │  │  Outbound Queue (read by WireGuard engine)                      │    │
/// │  │  [Packet] [Packet] [Packet] ...                                 │    │
/// │  └─────────────────────────────────────────────────────────────────┘    │
/// │                                                                          │
/// │  ┌─────────────────────────────────────────────────────────────────┐    │
/// │  │  Inbound Queue (written by WireGuard engine)                    │    │
/// │  │  [Packet] [Packet] [Packet] ...                                 │    │
/// │  └─────────────────────────────────────────────────────────────────┘    │
/// └─────────────────────────────────────────────────────────────────────────┘
/// ```
#[derive(Debug)]
pub struct VirtualTun {
    /// Device name.
    name: String,

    /// MTU.
    mtu: u16,

    /// Assigned IPv6 address.
    ipv6_addr: Option<Ipv6Addr>,

    /// Inbound packet queue (packets to be read by application).
    inbound: std::collections::VecDeque<Vec<u8>>,

    /// Outbound packet queue (packets written by application).
    outbound: std::collections::VecDeque<Vec<u8>>,

    /// Whether the device is closed.
    closed: bool,
}

impl VirtualTun {
    /// Create a new virtual TUN device.
    #[must_use]
    pub fn new(config: TunConfig) -> Self {
        let name = if config.name.is_empty() {
            format!("{DEFAULT_TUN_NAME}0")
        } else {
            config.name
        };

        Self {
            name,
            mtu: config.mtu,
            ipv6_addr: None,
            inbound: std::collections::VecDeque::new(),
            outbound: std::collections::VecDeque::new(),
            closed: false,
        }
    }

    /// Get device information.
    #[must_use]
    pub fn info(&self) -> TunInfo {
        TunInfo {
            name: self.name.clone(),
            mtu: self.mtu,
            is_virtual: true,
            ipv6_addr: self.ipv6_addr,
        }
    }

    /// Get the device name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the MTU.
    #[must_use]
    pub const fn mtu(&self) -> u16 {
        self.mtu
    }

    /// Set the IPv6 address.
    ///
    /// For virtual TUN, this just stores the address (no kernel configuration).
    pub fn set_ipv6(&mut self, addr: Ipv6Addr) -> Result<(), TunError> {
        if self.closed {
            return Err(TunError::Closed);
        }
        self.ipv6_addr = Some(addr);
        Ok(())
    }

    /// Read a packet from the virtual TUN (outbound queue).
    ///
    /// Returns the number of bytes read, or 0 if no packets are available.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, TunError> {
        if self.closed {
            return Err(TunError::Closed);
        }

        if let Some(packet) = self.outbound.pop_front() {
            let len = packet.len().min(buf.len());
            buf[..len].copy_from_slice(&packet[..len]);
            Ok(len)
        } else {
            // Non-blocking: return 0 when no packets available
            Ok(0)
        }
    }

    /// Write a packet to the virtual TUN (outbound queue).
    ///
    /// This queues a packet to be sent out through the WireGuard tunnel.
    pub fn write(&mut self, buf: &[u8]) -> Result<usize, TunError> {
        if self.closed {
            return Err(TunError::Closed);
        }

        self.outbound.push_back(buf.to_vec());
        Ok(buf.len())
    }

    /// Inject a packet into the inbound queue.
    ///
    /// This is called by the WireGuard engine when a packet is decrypted
    /// and needs to be delivered to the application.
    pub fn inject(&mut self, buf: &[u8]) -> Result<(), TunError> {
        if self.closed {
            return Err(TunError::Closed);
        }

        self.inbound.push_back(buf.to_vec());
        Ok(())
    }

    /// Take a packet from the inbound queue.
    ///
    /// Returns the decrypted packet data, or None if no packets are available.
    #[must_use]
    pub fn take_inbound(&mut self) -> Option<Vec<u8>> {
        self.inbound.pop_front()
    }

    /// Check if there are packets in the outbound queue.
    #[must_use]
    pub fn has_outbound(&self) -> bool {
        !self.outbound.is_empty()
    }

    /// Check if there are packets in the inbound queue.
    #[must_use]
    pub fn has_inbound(&self) -> bool {
        !self.inbound.is_empty()
    }

    /// Get the number of packets in the outbound queue.
    #[must_use]
    pub fn outbound_len(&self) -> usize {
        self.outbound.len()
    }

    /// Get the number of packets in the inbound queue.
    #[must_use]
    pub fn inbound_len(&self) -> usize {
        self.inbound.len()
    }

    /// Close the virtual TUN device.
    pub fn close(&mut self) {
        self.closed = true;
        self.inbound.clear();
        self.outbound.clear();
    }

    /// Check if the device is closed.
    #[must_use]
    pub const fn is_closed(&self) -> bool {
        self.closed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tun_config_defaults() {
        let config = TunConfig::default();

        assert!(config.name.is_empty());
        assert_eq!(config.mtu, DEFAULT_TUN_MTU);
        assert!(config.virtual_tun);
    }

    #[test]
    fn tun_config_builder() {
        let config = TunConfig::new().name("test0").mtu(1280).virtual_tun(false);

        assert_eq!(config.name, "test0");
        assert_eq!(config.mtu, 1280);
        assert!(!config.virtual_tun);
    }

    #[test]
    fn virtual_tun_create() {
        let config = TunConfig::new().name("virt0").mtu(1400);
        let tun = VirtualTun::new(config);

        assert_eq!(tun.name(), "virt0");
        assert_eq!(tun.mtu(), 1400);
        assert!(tun.ipv6_addr.is_none());
        assert!(!tun.is_closed());
    }

    #[test]
    fn virtual_tun_default_name() {
        let config = TunConfig::new();
        let tun = VirtualTun::new(config);

        assert_eq!(tun.name(), "moto0");
    }

    #[test]
    fn virtual_tun_set_ipv6() {
        let mut tun = VirtualTun::new(TunConfig::default());
        // Use actual hex representation: fd00:6d6f:746f:2::1 (moto = 6d6f:746f)
        let addr: Ipv6Addr = "fd00:6d6f:746f:2::1".parse().unwrap();

        tun.set_ipv6(addr).unwrap();

        assert_eq!(tun.ipv6_addr, Some(addr));
    }

    #[test]
    fn virtual_tun_write_read() {
        let mut tun = VirtualTun::new(TunConfig::default());

        // Write a packet
        let packet = [0x60, 0x00, 0x00, 0x00]; // IPv6 header start
        let written = tun.write(&packet).unwrap();
        assert_eq!(written, 4);
        assert!(tun.has_outbound());
        assert_eq!(tun.outbound_len(), 1);

        // Read the packet back
        let mut buf = [0u8; 64];
        let read = tun.read(&mut buf).unwrap();
        assert_eq!(read, 4);
        assert_eq!(&buf[..4], &packet);
        assert!(!tun.has_outbound());
    }

    #[test]
    fn virtual_tun_inject_take() {
        let mut tun = VirtualTun::new(TunConfig::default());

        // Inject a packet (simulating decrypted data from tunnel)
        let packet = vec![0x60, 0x00, 0x00, 0x00, 0x00, 0x10];
        tun.inject(&packet).unwrap();

        assert!(tun.has_inbound());
        assert_eq!(tun.inbound_len(), 1);

        // Take the packet
        let taken = tun.take_inbound().unwrap();
        assert_eq!(taken, packet);
        assert!(!tun.has_inbound());
    }

    #[test]
    fn virtual_tun_close() {
        let mut tun = VirtualTun::new(TunConfig::default());

        // Write some data
        tun.write(&[1, 2, 3]).unwrap();
        tun.inject(&[4, 5, 6]).unwrap();

        // Close
        tun.close();

        assert!(tun.is_closed());
        assert!(!tun.has_outbound());
        assert!(!tun.has_inbound());

        // Operations should fail after close
        assert!(matches!(tun.write(&[1, 2, 3]), Err(TunError::Closed)));
        assert!(matches!(tun.read(&mut [0u8; 64]), Err(TunError::Closed)));
        assert!(matches!(tun.inject(&[1, 2, 3]), Err(TunError::Closed)));
    }

    #[test]
    fn virtual_tun_read_empty() {
        let mut tun = VirtualTun::new(TunConfig::default());

        let mut buf = [0u8; 64];
        let read = tun.read(&mut buf).unwrap();

        // Should return 0 when no packets available
        assert_eq!(read, 0);
    }

    #[test]
    fn tun_device_virtual() {
        let config = TunConfig::new().virtual_tun(true);
        let mut tun = TunDevice::create(config).unwrap();

        assert!(tun.is_virtual());
        assert_eq!(tun.mtu(), DEFAULT_TUN_MTU);

        // Write and read
        tun.write(&[1, 2, 3, 4]).unwrap();
        let mut buf = [0u8; 64];
        let n = tun.read(&mut buf).unwrap();
        assert_eq!(n, 4);
        assert_eq!(&buf[..4], &[1, 2, 3, 4]);
    }

    #[test]
    fn tun_device_set_ipv6() {
        let config = TunConfig::new().virtual_tun(true);
        let mut tun = TunDevice::create(config).unwrap();
        // Use actual hex representation: fd00:6d6f:746f:2::42 (moto = 6d6f:746f)
        let addr: Ipv6Addr = "fd00:6d6f:746f:2::42".parse().unwrap();

        tun.set_ipv6(addr).unwrap();

        let info = tun.info();
        assert_eq!(info.ipv6_addr, Some(addr));
    }

    #[test]
    fn tun_info() {
        let config = TunConfig::new().name("test0").mtu(1280).virtual_tun(true);
        let tun = TunDevice::create(config).unwrap();

        let info = tun.info();
        assert_eq!(info.name, "test0");
        assert_eq!(info.mtu, 1280);
        assert!(info.is_virtual);
        assert!(info.ipv6_addr.is_none());
    }
}
