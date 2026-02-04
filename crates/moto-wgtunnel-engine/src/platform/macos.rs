//! macOS-specific TUN device implementation.
//!
//! This module provides TUN device support on macOS using the `utun` device
//! via the `SYSPROTO_CONTROL` system call. macOS uses a different mechanism
//! than Linux for TUN devices.
//!
//! # Requirements
//!
//! - macOS 10.10 or later
//! - Either root privileges or the application must be signed with the
//!   `com.apple.developer.networking.networkextension` entitlement
//!
//! # Example
//!
//! ```ignore
//! use moto_wgtunnel_engine::platform::macos::PlatformTun;
//! use moto_wgtunnel_engine::platform::TunConfig;
//!
//! let config = TunConfig::new().name("utun5");
//! let mut tun = PlatformTun::create(&config)?;
//!
//! // Read and write packets
//! let mut buf = [0u8; 2048];
//! let n = tun.read(&mut buf)?;
//! tun.write(&buf[..n])?;
//! ```
//!
//! # Packet Format
//!
//! Unlike Linux TUN, macOS utun devices prepend a 4-byte protocol family
//! header to each packet. This implementation handles the header automatically:
//! - Read: strips the header, returns raw IP packet
//! - Write: prepends the appropriate header based on IP version
//!
//! # Safety
//!
//! This module uses `unsafe` for system calls to the macOS kernel. All unsafe
//! blocks are documented with SAFETY comments explaining why they are sound.
#![allow(unsafe_code)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_possible_wrap)]

use std::ffi::CStr;
use std::mem;
use std::net::Ipv6Addr;
use std::os::unix::io::RawFd;

use super::{TunConfig, TunError, TunInfo};

/// Protocol family for IPv4.
const AF_INET: u32 = 2;

/// Protocol family for IPv6.
const AF_INET6: u32 = 30;

/// System protocol for control sockets.
const SYSPROTO_CONTROL: libc::c_int = 2;

/// Socket option level for system protocols.
const SYSPROTO_SYSTEM: libc::c_int = 0x800;

/// Control name for utun.
const UTUN_CONTROL_NAME: &[u8] = b"com.apple.net.utun_control\0";

/// Maximum control name length.
const MAX_KCTL_NAME: usize = 96;

/// ioctl request to get control ID.
const CTLIOCGINFO: libc::c_ulong = 0xc064_4e03;

/// ioctl request to set interface flags.
const SIOCSIFFLAGS: libc::c_ulong = 0x8020_6910;

/// ioctl request to get interface flags.
const SIOCGIFFLAGS: libc::c_ulong = 0xc020_6911;

/// ioctl request to set MTU.
const SIOCSIFMTU: libc::c_ulong = 0x8020_6934;

/// Interface flag: UP.
const IFF_UP: i16 = 0x1;

/// Interface flag: RUNNING.
const IFF_RUNNING: i16 = 0x40;

/// Maximum interface name length.
const IFNAMSIZ: usize = 16;

/// Control socket info structure.
#[repr(C)]
struct CtlInfo {
    ctl_id: u32,
    ctl_name: [u8; MAX_KCTL_NAME],
}

/// Control socket address structure.
#[repr(C)]
struct SockaddrCtl {
    sc_len: u8,
    sc_family: u8,
    ss_sysaddr: u16,
    sc_id: u32,
    sc_unit: u32,
    sc_reserved: [u32; 5],
}

/// Interface request structure for macOS.
#[repr(C)]
struct IfReq {
    ifr_name: [libc::c_char; IFNAMSIZ],
    ifr_data: IfReqData,
}

/// Union-like data for ifreq.
#[repr(C)]
union IfReqData {
    ifr_flags: i16,
    ifr_mtu: i32,
    _padding: [u8; 16],
}

/// macOS-specific TUN device using utun.
#[derive(Debug)]
pub struct PlatformTun {
    /// The file descriptor (wrapped).
    fd: Option<RawFd>,

    /// Device name (e.g., "utun5").
    name: String,

    /// MTU.
    mtu: u16,

    /// Assigned IPv6 address.
    ipv6_addr: Option<Ipv6Addr>,

    /// Unit number (for utun device).
    unit: u32,
}

impl PlatformTun {
    /// Create a new TUN device.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Control socket cannot be created
    /// - Device cannot be configured
    pub fn create(config: &TunConfig) -> Result<Self, TunError> {
        // Create a system control socket
        // SAFETY: libc::socket is a standard POSIX syscall. Returns -1 on error which we check.
        let fd = unsafe { libc::socket(libc::PF_SYSTEM, libc::SOCK_DGRAM, SYSPROTO_CONTROL) };
        if fd < 0 {
            let err = std::io::Error::last_os_error();
            return Err(TunError::Create(format!(
                "failed to create control socket: {err}"
            )));
        }

        // Get the control ID for utun
        // SAFETY: mem::zeroed is safe for CtlInfo which is a repr(C) struct with only
        // primitive types (u32, [u8; N]) that have valid zero representations.
        let mut info: CtlInfo = unsafe { mem::zeroed() };
        info.ctl_name[..UTUN_CONTROL_NAME.len()].copy_from_slice(UTUN_CONTROL_NAME);

        // SAFETY: libc::ioctl with CTLIOCGINFO queries control info. fd is valid (checked above),
        // and info is properly initialized. Returns < 0 on error which we check.
        if unsafe { libc::ioctl(fd, CTLIOCGINFO, &mut info) } < 0 {
            let err = std::io::Error::last_os_error();
            // SAFETY: fd is a valid file descriptor from socket() above.
            unsafe { libc::close(fd) };
            return Err(TunError::Create(format!(
                "failed to get utun control ID: {err}"
            )));
        }

        // Parse the requested unit number from the name, or use 0 for auto-assign
        let unit = if config.name.is_empty() {
            0 // Let kernel assign
        } else if let Some(stripped) = config.name.strip_prefix("utun") {
            stripped.parse::<u32>().unwrap_or(0)
        } else {
            0
        };

        // Prepare the control socket address
        // SAFETY for casts: SockaddrCtl is a fixed-size struct (32 bytes on macOS).
        // AF_SYSTEM and SYSPROTO_SYSTEM are small constants that fit in u8/u16.
        let addr = SockaddrCtl {
            sc_len: mem::size_of::<SockaddrCtl>() as u8, // Always 32, fits in u8
            sc_family: libc::AF_SYSTEM as u8,            // Always 32, fits in u8
            ss_sysaddr: SYSPROTO_SYSTEM as u16,          // Always 0x800, fits in u16
            sc_id: info.ctl_id,
            sc_unit: unit + 1, // utun unit is 1-indexed in connect, but 0-indexed in name
            sc_reserved: [0; 5],
        };

        // Connect to create the utun device
        // SAFETY: libc::connect is a standard POSIX syscall. fd is valid, addr is a properly
        // initialized SockaddrCtl struct, and we pass its exact size. Returns < 0 on error.
        // The size cast is safe because SockaddrCtl is a fixed-size struct (32 bytes).
        if unsafe {
            libc::connect(
                fd,
                (&raw const addr).cast(),
                mem::size_of::<SockaddrCtl>() as libc::socklen_t,
            )
        } < 0
        {
            let err = std::io::Error::last_os_error();
            // SAFETY: fd is a valid file descriptor from socket() above.
            unsafe { libc::close(fd) };
            return Err(TunError::Create(format!(
                "failed to connect utun device: {err}"
            )));
        }

        // Get the actual device name
        let mut name_buf = [0u8; IFNAMSIZ];
        // SAFETY: IFNAMSIZ is 16, which fits in u32 for socklen_t.
        let mut name_len = IFNAMSIZ as libc::socklen_t;

        // SAFETY: libc::getsockopt is a standard POSIX syscall. fd is valid, name_buf is a valid
        // buffer of IFNAMSIZ bytes, and name_len is initialized to IFNAMSIZ. Returns < 0 on error.
        if unsafe {
            libc::getsockopt(
                fd,
                SYSPROTO_CONTROL,
                2, // UTUN_OPT_IFNAME
                name_buf.as_mut_ptr().cast(),
                &raw mut name_len,
            )
        } < 0
        {
            let err = std::io::Error::last_os_error();
            // SAFETY: fd is a valid file descriptor from socket() above.
            unsafe { libc::close(fd) };
            return Err(TunError::Create(format!(
                "failed to get utun device name: {err}"
            )));
        }

        // SAFETY: getsockopt with UTUN_OPT_IFNAME writes a null-terminated C string into name_buf.
        // We checked the return value above, so name_buf contains valid UTF-8 interface name.
        let actual_name = unsafe {
            CStr::from_ptr(name_buf.as_ptr().cast())
                .to_string_lossy()
                .into_owned()
        };

        // Extract the actual unit number from the name
        let actual_unit = actual_name
            .strip_prefix("utun")
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);

        let mut tun = Self {
            fd: Some(fd),
            name: actual_name,
            mtu: config.mtu,
            ipv6_addr: None,
            unit: actual_unit,
        };

        // Set MTU
        tun.set_mtu(config.mtu)?;

        // Bring up the interface
        tun.bring_up()?;

        Ok(tun)
    }

    /// Get device information.
    #[must_use]
    pub fn info(&self) -> TunInfo {
        TunInfo {
            name: self.name.clone(),
            mtu: self.mtu,
            is_virtual: false,
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

    /// Get the unit number.
    #[must_use]
    pub const fn unit(&self) -> u32 {
        self.unit
    }

    /// Set the MTU.
    fn set_mtu(&mut self, mtu: u16) -> Result<(), TunError> {
        // SAFETY: libc::socket is a standard POSIX syscall. Returns -1 on error which we check.
        let sock = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0) };
        if sock < 0 {
            return Err(TunError::Configure(
                "failed to create socket for MTU ioctl".to_string(),
            ));
        }

        // SAFETY: mem::zeroed is safe for IfReq which is a repr(C) struct containing only
        // primitive types and a union of primitives. All have valid zero representations.
        let mut ifr: IfReq = unsafe { mem::zeroed() };
        for (i, c) in self.name.bytes().enumerate() {
            if i >= IFNAMSIZ - 1 {
                break;
            }
            // Interface names are ASCII, so u8 to c_char (i8 on macOS) is safe for printable ASCII.
            ifr.ifr_name[i] = c as libc::c_char;
        }
        ifr.ifr_data.ifr_mtu = i32::from(mtu);

        // SAFETY: libc::ioctl with SIOCSIFMTU sets the interface MTU. sock is valid,
        // and ifr is properly initialized with the interface name and MTU value.
        let result = unsafe { libc::ioctl(sock, SIOCSIFMTU, &ifr) };
        // SAFETY: sock is a valid file descriptor from socket() above.
        unsafe { libc::close(sock) };

        if result < 0 {
            let err = std::io::Error::last_os_error();
            return Err(TunError::Configure(format!("failed to set MTU: {err}")));
        }

        self.mtu = mtu;
        Ok(())
    }

    /// Bring up the interface.
    fn bring_up(&self) -> Result<(), TunError> {
        // SAFETY: libc::socket is a standard POSIX syscall. Returns -1 on error which we check.
        let sock = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0) };
        if sock < 0 {
            return Err(TunError::Configure(
                "failed to create socket for interface ioctl".to_string(),
            ));
        }

        // SAFETY: mem::zeroed is safe for IfReq which is a repr(C) struct containing only
        // primitive types and a union of primitives. All have valid zero representations.
        let mut ifr: IfReq = unsafe { mem::zeroed() };
        for (i, c) in self.name.bytes().enumerate() {
            if i >= IFNAMSIZ - 1 {
                break;
            }
            // Interface names are ASCII, so u8 to c_char (i8 on macOS) is safe for printable ASCII.
            ifr.ifr_name[i] = c as libc::c_char;
        }

        // Get current flags
        // SAFETY: libc::ioctl with SIOCGIFFLAGS retrieves interface flags. sock is valid,
        // and ifr is properly initialized with the interface name. Returns < 0 on error.
        if unsafe { libc::ioctl(sock, SIOCGIFFLAGS, &mut ifr) } < 0 {
            let err = std::io::Error::last_os_error();
            // SAFETY: sock is a valid file descriptor from socket() above.
            unsafe { libc::close(sock) };
            return Err(TunError::Configure(format!(
                "failed to get interface flags: {err}"
            )));
        }

        // Set UP and RUNNING flags
        // SAFETY: We just read ifr_flags via SIOCGIFFLAGS above, so the union is in the ifr_flags
        // variant. We're modifying the same field to set additional flags.
        unsafe {
            ifr.ifr_data.ifr_flags |= IFF_UP | IFF_RUNNING;
        }

        // Apply flags
        // SAFETY: libc::ioctl with SIOCSIFFLAGS sets interface flags. sock is valid,
        // and ifr contains valid flags we just read and modified.
        let result = unsafe { libc::ioctl(sock, SIOCSIFFLAGS, &ifr) };
        // SAFETY: sock is a valid file descriptor from socket() above.
        unsafe { libc::close(sock) };

        if result < 0 {
            let err = std::io::Error::last_os_error();
            return Err(TunError::Configure(format!(
                "failed to bring up interface: {err}"
            )));
        }

        Ok(())
    }

    /// Set the IPv6 address.
    ///
    /// # Note
    ///
    /// This requires root privileges.
    ///
    /// # Errors
    ///
    /// Returns error if ifconfig command fails or address cannot be assigned.
    pub fn set_ipv6(&mut self, addr: Ipv6Addr) -> Result<(), TunError> {
        // Use ifconfig command on macOS
        let output = std::process::Command::new("ifconfig")
            .args([&self.name, "inet6", &format!("{addr}/128"), "alias"])
            .output()
            .map_err(|e| TunError::Configure(format!("failed to run ifconfig: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Ignore "already exists" error
            if !stderr.contains("already exists") {
                return Err(TunError::Configure(format!(
                    "failed to set IPv6 address: {stderr}"
                )));
            }
        }

        self.ipv6_addr = Some(addr);
        Ok(())
    }

    /// Read a packet from the TUN device.
    ///
    /// This strips the 4-byte protocol family header that macOS utun prepends.
    ///
    /// # Errors
    ///
    /// Returns error if read fails or device is closed.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, TunError> {
        let fd = self.fd.ok_or(TunError::Closed)?;

        // Read with space for the 4-byte header
        let mut full_buf = vec![0u8; buf.len() + 4];
        // SAFETY: libc::read is a standard POSIX syscall. fd is valid (checked via ok_or above),
        // full_buf is a valid mutable buffer, and we pass its exact length.
        let n = unsafe { libc::read(fd, full_buf.as_mut_ptr().cast(), full_buf.len()) };

        if n < 0 {
            return Err(TunError::Read(std::io::Error::last_os_error()));
        }

        // Cast is safe: we just checked n >= 0, and the returned byte count
        // cannot exceed the buffer size which fits in usize.
        let n = n as usize;
        if n <= 4 {
            return Ok(0); // No payload
        }

        // Strip the 4-byte header and copy to output buffer
        let payload_len = n - 4;
        let copy_len = payload_len.min(buf.len());
        buf[..copy_len].copy_from_slice(&full_buf[4..4 + copy_len]);

        Ok(copy_len)
    }

    /// Write a packet to the TUN device.
    ///
    /// This prepends the 4-byte protocol family header that macOS utun expects.
    ///
    /// # Errors
    ///
    /// Returns error if write fails or device is closed.
    pub fn write(&mut self, buf: &[u8]) -> Result<usize, TunError> {
        let fd = self.fd.ok_or(TunError::Closed)?;

        if buf.is_empty() {
            return Ok(0);
        }

        // Determine protocol family from IP version
        let proto = match buf[0] >> 4 {
            4 => AF_INET,
            6 => AF_INET6,
            _ => {
                return Err(TunError::Write(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "unknown IP version",
                )));
            }
        };

        // Prepend the 4-byte header
        let mut full_buf = Vec::with_capacity(buf.len() + 4);
        full_buf.extend_from_slice(&proto.to_be_bytes());
        full_buf.extend_from_slice(buf);

        // SAFETY: libc::write is a standard POSIX syscall. fd is valid (checked via ok_or above),
        // full_buf is a valid buffer, and we pass its exact length.
        let n = unsafe { libc::write(fd, full_buf.as_ptr().cast(), full_buf.len()) };

        if n < 0 {
            return Err(TunError::Write(std::io::Error::last_os_error()));
        }

        // Return the number of payload bytes written (excluding header).
        // Cast is safe: we just checked n >= 0, and the byte count cannot exceed buffer size.
        let written = (n as usize).saturating_sub(4);
        Ok(written)
    }

    /// Close the TUN device.
    pub fn close(&mut self) {
        if let Some(fd) = self.fd.take() {
            // SAFETY: fd came from a successful socket() + connect() sequence,
            // and we only close it once via take().
            unsafe { libc::close(fd) };
        }
    }
}

impl Drop for PlatformTun {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // These tests require root, so they're ignored by default.
    // Run with: sudo cargo test --package moto-wgtunnel-engine -- --ignored

    #[test]
    #[ignore = "requires root"]
    fn create_utun_device() {
        let config = TunConfig::new().mtu(1420);
        let tun = PlatformTun::create(&config).unwrap();

        assert!(tun.name().starts_with("utun"));
        assert_eq!(tun.mtu(), 1420);
    }

    #[test]
    fn structures_size() {
        // Verify struct sizes are reasonable
        assert!(mem::size_of::<CtlInfo>() > 0);
        assert!(mem::size_of::<SockaddrCtl>() >= 32);
        assert!(mem::size_of::<IfReq>() >= 32);
    }

    #[test]
    fn protocol_family_values() {
        // Verify protocol family constants
        assert_eq!(AF_INET, 2);
        assert_eq!(AF_INET6, 30);
    }
}
