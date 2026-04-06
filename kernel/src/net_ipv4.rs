// SPDX-License-Identifier: MIT OR Apache-2.0

//! Runtime IPv4 triple (guest / gateway / DNS) and QEMU SLIRP defaults.

/// Guest, default gateway, and DNS used by `NetStack` (ARP, DNS, TCP).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct NetIpv4Addrs {
    pub our: [u8; 4],
    pub gw: [u8; 4],
    pub dns: [u8; 4],
}

impl NetIpv4Addrs {
    pub const SLIRP: Self = Self {
        our: [10, 0, 2, 15],
        gw: [10, 0, 2, 2],
        dns: [10, 0, 2, 3],
    };

    pub const ZERO: Self = Self {
        our: [0, 0, 0, 0],
        gw: [0, 0, 0, 0],
        dns: [0, 0, 0, 0],
    };

    pub fn is_our_zero(self) -> bool {
        self.our == [0, 0, 0, 0]
    }
}
