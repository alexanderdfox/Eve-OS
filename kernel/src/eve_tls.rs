// SPDX-License-Identifier: MIT OR Apache-2.0

//! TLS 1.3 (HTTPS) over the in-kernel TCP stack: `embedded_io` bridge + PRNG.
//!
//! **Server identity is not verified** (`UnsecureProvider`): the session is encrypted (TLS 1.3) but
//! a MITM can present any certificate. Full PKIX with `rustls-webpki` is not linked here because
//! **`ring` does not compile** for the `x86_64-unknown-none` kernel target (no hosted C library).
//! Use only networks you trust, or treat HTTPS as transport encryption only.

use core::fmt;

use embedded_io::{ErrorKind, ErrorType, Read, Write};
use rand_core::{CryptoRng, RngCore, impls};

use crate::net::NetStack;

#[derive(Debug, Clone, Copy)]
pub struct NetIoError;

impl fmt::Display for NetIoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("net io")
    }
}

impl core::error::Error for NetIoError {}

impl embedded_io::Error for NetIoError {
    fn kind(&self) -> ErrorKind {
        ErrorKind::Other
    }
}

/// Feeds ciphertext between `embedded-tls` and [`NetStack`] TLS queues (see `net.rs`).
pub struct TlsNetBridge {
    pub net: *mut NetStack,
}

impl ErrorType for TlsNetBridge {
    type Error = NetIoError;
}

impl Read for TlsNetBridge {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.net.is_null() {
            return Err(NetIoError);
        }
        let spin_max: u32 = 2_000_000;
        let mut spins: u32 = 0;
        unsafe {
            let net = &mut *self.net;
            loop {
                let n = net.tls_cipher_pop(buf);
                if n > 0 {
                    return Ok(n);
                }
                if net.tls_eof_from_tcp() {
                    return Ok(0);
                }
                net.tls_spin_poll();
                spins = spins.wrapping_add(1);
                if spins >= spin_max {
                    return Err(NetIoError);
                }
            }
        }
    }
}

impl Write for TlsNetBridge {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        if self.net.is_null() {
            return Err(NetIoError);
        }
        unsafe {
            if (*self.net).tls_cipher_tx_append(buf) {
                Ok(buf.len())
            } else {
                Err(NetIoError)
            }
        }
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        if self.net.is_null() {
            return Err(NetIoError);
        }
        unsafe {
            (*self.net).tls_note_flush_pending();
        }
        Ok(())
    }
}

/// Small deterministic-ish RNG for TLS key material (seed from MAC + tick in `NetStack::seed_from_mac`).
pub struct EveRng(u64);

impl EveRng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }
}

impl CryptoRng for EveRng {}

impl RngCore for EveRng {
    fn next_u32(&mut self) -> u32 {
        impls::next_u32_via_fill(self)
    }

    fn next_u64(&mut self) -> u64 {
        impls::next_u64_via_fill(self)
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        for b in dest.iter_mut() {
            // xorshift64*
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            *b = ((x.wrapping_mul(0x2545_F491_4F6C_DD1D)) >> 56) as u8;
        }
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

