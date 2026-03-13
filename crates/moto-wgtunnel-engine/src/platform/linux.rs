//! Linux-specific TUN device implementation.
//!
//! This module provides TUN device support on Linux using the `/dev/net/tun`
//! character device. The implementation uses the `IFF_TUN` and `IFF_NO_PI`
//! flags for raw IP packet I/O without the packet information header.
//!
//! # Requirements
//!
//! - `/dev/net/tun` must exist (typically provided by the `tun` kernel module)
//! - Either root privileges or `CAP_NET_ADMIN` capability
//!
//! # Example
//!
//! ```ignore
//! use moto_wgtunnel_engine::platform::linux::PlatformTun;
//! use moto_wgtunnel_engine::platform::TunConfig;
//!
//! let config = TunConfig::new().name("moto0");
//! let mut tun = PlatformTun::create(config)?;
//!
//! // Read and write packets
//! let mut buf = [0u8; 2048];
//! let n = tun.read(&mut buf)?;
//! tun.write(&buf[..n])?;
//! ```

use std::ffi::CStr;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::mem;
use std::net::Ipv6Addr;
use std::os::unix::io::AsRawFd;

use super::{DEFAULT_TUN_NAME, TunConfig, TunError, TunInfo};

/// Path to the TUN clone device.
const TUN_DEVICE_PATH: &str = "/dev/net/tun";

/// Maximum interface name length (IFNAMSIZ in Linux).
const IFNAMSIZ: usize = 16;

/// TUN device flag (layer 3 device).
const IFF_TUN: libc::c_short = 0x0001;

/// No packet information header flag.
const IFF_NO_PI: libc::c_short = 0x1000;

/// ioctl request to create TUN device.
const TUNSETIFF: libc::c_ulong = 0x4004_54ca;

/// ioctl request to set interface flags (bring up).
const SIOCSIFFLAGS: libc::c_ulong = 0x8914;

/// ioctl request to get interface flags.
const SIOCGIFFLAGS: libc::c_ulong = 0x8913;

/// ioctl request to set MTU.
const SIOCSIFMTU: libc::c_ulong = 0x8922;

/// Interface flag: UP.
const IFF_UP: libc::c_short = 0x1;

/// Interface flag: RUNNING.
const IFF_RUNNING: libc::c_short = 0x40;

/// Linux interface request structure.
#[repr(C)]
struct IfReq {
    ifr_name: [libc::c_char; IFNAMSIZ],
    ifr_data: IfReqData,
}

/// Union-like data for ifreq (we use different fields for different ioctls).
#[repr(C)]
union IfReqData {
    ifr_flags: libc::c_short,
    ifr_mtu: libc::c_int,
    _padding: [u8; 24],
}

/// Linux-specific TUN device.
#[derive(Debug)]
pub struct PlatformTun {
    /// The TUN file descriptor.
    file: Option<File>,

    /// Device name.
    name: String,

    /// MTU.
    mtu: u16,

    /// Assigned IPv6 address.
    ipv6_addr: Option<Ipv6Addr>,
}

impl PlatformTun {
    /// Create a new TUN device.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - `/dev/net/tun` cannot be opened
    /// - ioctl fails (permission denied, invalid name, etc.)
    pub fn create(config: &TunConfig) -> Result<Self, TunError> {
        // Open the TUN clone device
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(TUN_DEVICE_PATH)
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    TunError::NotAvailable(format!(
                        "{TUN_DEVICE_PATH} not found. Is the tun kernel module loaded?"
                    ))
                } else if e.kind() == std::io::ErrorKind::PermissionDenied {
                    TunError::PermissionDenied(format!(
                        "Cannot open {TUN_DEVICE_PATH}. Try running with sudo or CAP_NET_ADMIN."
                    ))
                } else {
                    TunError::Create(e.to_string())
                }
            })?;

        // Prepare the interface name
        let name = if config.name.is_empty() {
            format!("{DEFAULT_TUN_NAME}%d") // Let kernel assign a number
        } else {
            config.name.clone()
        };

        if name.len() >= IFNAMSIZ {
            return Err(TunError::Configure(format!(
                "interface name too long (max {} chars)",
                IFNAMSIZ - 1
            )));
        }

        // Prepare ifreq structure
        let mut ifr: IfReq = unsafe { mem::zeroed() };

        // Copy name to ifr_name (null-terminated)
        for (i, c) in name.bytes().enumerate() {
            if i >= IFNAMSIZ - 1 {
                break;
            }
            ifr.ifr_name[i] = c as libc::c_char;
        }

        // Set flags: TUN device, no packet info header
        ifr.ifr_data.ifr_flags = IFF_TUN | IFF_NO_PI;

        // Create the TUN device
        let fd = file.as_raw_fd();
        let result = unsafe { libc::ioctl(fd, TUNSETIFF, &mut ifr) };

        if result < 0 {
            let err = std::io::Error::last_os_error();
            return Err(TunError::Create(format!("TUNSETIFF ioctl failed: {err}")));
        }

        // Get the actual device name (kernel may have assigned a number)
        let actual_name = unsafe {
            CStr::from_ptr(ifr.ifr_name.as_ptr())
                .to_string_lossy()
                .into_owned()
        };

        let mut tun = Self {
            file: Some(file),
            name: actual_name,
            mtu: config.mtu,
            ipv6_addr: None,
        };

        // Set MTU if different from default
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

    /// Set the MTU.
    fn set_mtu(&mut self, mtu: u16) -> Result<(), TunError> {
        // Create a socket for the ioctl
        let sock = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0) };
        if sock < 0 {
            return Err(TunError::Configure(
                "failed to create socket for MTU ioctl".to_string(),
            ));
        }

        // Prepare ifreq
        let mut ifr: IfReq = unsafe { mem::zeroed() };
        for (i, c) in self.name.bytes().enumerate() {
            if i >= IFNAMSIZ - 1 {
                break;
            }
            ifr.ifr_name[i] = c as libc::c_char;
        }
        ifr.ifr_data.ifr_mtu = i32::from(mtu);

        // Set MTU
        let result = unsafe { libc::ioctl(sock, SIOCSIFMTU, &ifr) };
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
        // Create a socket for the ioctl
        let sock = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0) };
        if sock < 0 {
            return Err(TunError::Configure(
                "failed to create socket for interface ioctl".to_string(),
            ));
        }

        // Prepare ifreq
        let mut ifr: IfReq = unsafe { mem::zeroed() };
        for (i, c) in self.name.bytes().enumerate() {
            if i >= IFNAMSIZ - 1 {
                break;
            }
            ifr.ifr_name[i] = c as libc::c_char;
        }

        // Get current flags
        let result = unsafe { libc::ioctl(sock, SIOCGIFFLAGS, &mut ifr) };
        if result < 0 {
            let err = std::io::Error::last_os_error();
            unsafe { libc::close(sock) };
            return Err(TunError::Configure(format!(
                "failed to get interface flags: {err}"
            )));
        }

        // Set UP and RUNNING flags
        unsafe {
            ifr.ifr_data.ifr_flags |= IFF_UP | IFF_RUNNING;
        }

        // Apply flags
        let result = unsafe { libc::ioctl(sock, SIOCSIFFLAGS, &ifr) };
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
    /// # Errors
    ///
    /// Returns error if the `ip` command fails or the address cannot be set.
    ///
    /// # Note
    ///
    /// This requires root privileges or `CAP_NET_ADMIN`.
    pub fn set_ipv6(&mut self, addr: Ipv6Addr) -> Result<(), TunError> {
        // Use ip command to set the address (simpler than netlink)
        let output = std::process::Command::new("ip")
            .args(["addr", "add", &format!("{addr}/128"), "dev", &self.name])
            .output()
            .map_err(|e| TunError::Configure(format!("failed to run ip command: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Ignore "already exists" error
            if !stderr.contains("File exists") {
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
    /// # Errors
    ///
    /// Returns error if read fails or device is closed.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, TunError> {
        let file = self.file.as_mut().ok_or(TunError::Closed)?;
        file.read(buf).map_err(TunError::Read)
    }

    /// Write a packet to the TUN device.
    ///
    /// # Errors
    ///
    /// Returns error if write fails or device is closed.
    pub fn write(&mut self, buf: &[u8]) -> Result<usize, TunError> {
        let file = self.file.as_mut().ok_or(TunError::Closed)?;
        file.write(buf).map_err(TunError::Write)
    }

    /// Close the TUN device.
    pub fn close(&mut self) {
        self.file = None;
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

    // These tests require root and /dev/net/tun, so they're ignored by default.
    // Run with: cargo test --package moto-wgtunnel-engine -- --ignored

    #[test]
    #[ignore = "requires root and /dev/net/tun"]
    fn create_tun_device() {
        let config = TunConfig::new().name("mototest%d").mtu(1420);
        let tun = PlatformTun::create(&config).unwrap();

        assert!(tun.name().starts_with("mototest"));
        assert_eq!(tun.mtu(), 1420);
    }

    #[test]
    #[ignore = "requires root and /dev/net/tun"]
    fn tun_read_write() {
        let config = TunConfig::new().name("mototest%d");
        let mut tun = PlatformTun::create(&config).unwrap();

        // Write an IPv6 packet
        let packet = [
            0x60, 0x00, 0x00, 0x00, // Version, traffic class, flow label
            0x00, 0x00, 0x00,
            0x00, // Payload length, next header, hop limit
                  // ... rest of packet
        ];
        let written = tun.write(&packet).unwrap();
        assert_eq!(written, packet.len());
    }

    #[test]
    fn ifreq_size() {
        // Verify our struct matches the kernel's expectations
        assert!(mem::size_of::<IfReq>() >= 32); // Minimum ifreq size
    }
}
