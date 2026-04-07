// SPDX-License-Identifier: MIT OR Apache-2.0

//! TLS 1.3 (HTTPS) over the in-kernel TCP stack: `embedded_io` bridge + PRNG + rustpki verifier.

use core::fmt;

use embedded_tls::blocking::{Aes128GcmSha256, CryptoProvider, TlsClock, TlsError, TlsVerifier};
use embedded_tls::SignatureScheme;
use embedded_tls::pki::CertVerifier;
use embedded_io::{ErrorKind, ErrorType, Read, Write};
use p256::SecretKey;
use p256::ecdsa::{DerSignature, SigningKey};
use rand_core::{CryptoRng, RngCore, impls};
use signature::SignerMut;

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

/// Build-time UNIX epoch (seconds), emitted by `kernel/build.rs`.
const BUILD_EPOCH_STR: &str = env!("EVE_BUILD_UNIX_EPOCH");

fn parse_u64_ascii(s: &str) -> Option<u64> {
    let mut out = 0u64;
    for b in s.as_bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        out = out.saturating_mul(10).saturating_add(u64::from(*b - b'0'));
    }
    Some(out)
}

pub struct EveTlsClock;

impl TlsClock for EveTlsClock {
    fn now() -> Option<u64> {
        parse_u64_ascii(BUILD_EPOCH_STR)
    }
}

/// Root trust anchors (DER) shipped with Eve.
pub const ISRG_ROOT_X1_DER: &[u8] = include_bytes!("../certs/isrg_root_x1.der");
pub const DIGICERT_GLOBAL_ROOT_G2_DER: &[u8] = include_bytes!("../certs/digicert_global_root_g2.der");

pub struct EveVerifiedTlsProvider {
    rng: EveRng,
    verifier: CertVerifier<Aes128GcmSha256, EveTlsClock, 4096>,
}

impl EveVerifiedTlsProvider {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: EveRng::new(seed),
            verifier: CertVerifier::new(),
        }
    }
}

impl CryptoProvider for EveVerifiedTlsProvider {
    type CipherSuite = Aes128GcmSha256;
    type Signature = DerSignature;

    fn rng(&mut self) -> impl rand_core::CryptoRngCore {
        &mut self.rng
    }

    fn verifier(&mut self) -> Result<&mut impl TlsVerifier<Aes128GcmSha256>, TlsError> {
        Ok(&mut self.verifier)
    }

    fn signer(
        &mut self,
        key_der: &[u8],
    ) -> Result<(impl SignerMut<Self::Signature>, SignatureScheme), TlsError> {
        let secret_key =
            SecretKey::from_sec1_der(key_der).map_err(|_| TlsError::InvalidPrivateKey)?;
        Ok((
            SigningKey::from(&secret_key),
            SignatureScheme::EcdsaSecp256r1Sha256,
        ))
    }
}

