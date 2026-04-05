// SPDX-License-Identifier: MIT OR Apache-2.0

//! ARP, DNS (UDP to QEMU `10.0.2.3`), and minimal TCP/HTTP/1.0 client for user NAT (`10.0.2.0/24`).
//! **`https://`** uses TLS 1.3 via `embedded-tls` (**encrypted**; **certificates not verified** on
//! bare metal — see `eve_tls.rs` and `utm/BROWSER-LIMITS.txt`).
//!
//! **Bare metal:** guest IP, gateway, and DNS below are fixed for **QEMU `-netdev user`**.
//! Real LANs need future DHCP or configurable static addresses and a non-VirtIO NIC driver — see
//! `install/REAL-HARDWARE.txt`.

use core::mem::MaybeUninit;

use crate::eve_tls::{EveRng, TlsNetBridge};
use crate::url::parse_fetch_url;
use crate::virtio_net::VirtioNet;
use embedded_io::Write as _;
use embedded_tls::blocking::{Aes128GcmSha256, TlsConfig, TlsConnection, TlsContext, UnsecureProvider};

pub const VIRTIO_NET_HDR: usize = 12;

const TLS_CIPHER_RX_CAP: usize = 49152;
const TLS_TX_CAP: usize = 24576;

const OUR_IP: [u8; 4] = [10, 0, 2, 15];
const GW_IP: [u8; 4] = [10, 0, 2, 2];
const DNS_SERVER: [u8; 4] = [10, 0, 2, 3];

const LOCAL_PORT: u16 = 49152;
const DNS_LOCAL_PORT: u16 = 53000;

const PAGE_CAP: usize = 12288;
const STREAM_CAP: usize = 4096;

const ETH_P_IP: u16 = 0x0800;
const ETH_P_ARP: u16 = 0x0806;
const ARP_OP_REQ: u16 = 1;
const ARP_OP_REPLY: u16 = 2;
const ARP_HTYPE_ETH: u16 = 1;
const ARP_PTYPE_IP: u16 = 0x0800;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NetPhase {
    Off,
    Arp,
    Dns,
    Tcp,
    Http,
    Done,
}

pub struct NetStack {
    gw_mac: [u8; 6],
    gw_known: bool,
    tick: u32,
    ip_id: u16,
    tcp_seq: u32,
    tcp_ack: u32,
    syn_sent: bool,
    get_sent: bool,
    syn_retries: u8,
    pub phase: NetPhase,
    /// Total TCP payload bytes received for this fetch (status line).
    pub http_bytes: u32,
    pub page: [u8; PAGE_CAP],
    pub page_len: usize,
    pub page_gen: u32,
    pub fetch_err: [u8; 80],
    pub fetch_err_len: usize,

    fetch_armed: bool,
    fetch_done: bool,
    needs_dns: bool,
    dns_done: bool,
    dns_tx_id: u16,
    dns_name: [u8; 96],
    dns_name_len: usize,
    dns_xmit_phase: u32,

    remote_ip: [u8; 4],
    remote_port: u16,
    host_header: [u8; 96],
    host_header_len: usize,
    path: [u8; 160],
    path_len: usize,

    https_mode: bool,
    tls_handshake_done: bool,
    https_handshake_queued: bool,
    tls_live: bool,
    tls_tcp_eof: bool,
    tls_tx_flush_pending: bool,

    tls_cipher_in: [u8; TLS_CIPHER_RX_CAP],
    tls_cipher_off: usize,
    tls_cipher_len: usize,

    tls_tx_buf: [u8; TLS_TX_CAP],
    tls_tx_len: usize,

    tls_poll_vio: *mut VirtioNet,
    tls_poll_mac: [u8; 6],
    tls_poll_scratch: *mut u8,
    tls_poll_scratch_len: usize,

    tls_rbuf: [u8; 16384],
    tls_wbuf: [u8; 16384],
    tls: MaybeUninit<TlsConnection<'static, TlsNetBridge, Aes128GcmSha256>>,
    tls_server_name: [u8; 96],
    tls_server_name_len: usize,

    stream: [u8; STREAM_CAP],
    stream_len: usize,
    header_found: bool,
    pub page_truncated: bool,
}

impl NetStack {
    pub fn new() -> Self {
        Self {
            gw_mac: [0; 6],
            gw_known: false,
            tick: 0,
            ip_id: 0x4000,
            tcp_seq: 0,
            tcp_ack: 0,
            syn_sent: false,
            get_sent: false,
            syn_retries: 0,
            phase: NetPhase::Off,
            http_bytes: 0,
            page: [0; PAGE_CAP],
            page_len: 0,
            page_gen: 0,
            fetch_err: [0; 80],
            fetch_err_len: 0,
            fetch_armed: false,
            fetch_done: false,
            needs_dns: false,
            dns_done: false,
            dns_tx_id: 0,
            dns_name: [0; 96],
            dns_name_len: 0,
            dns_xmit_phase: 0,
            remote_ip: [0; 4],
            remote_port: 80,
            host_header: [0; 96],
            host_header_len: 0,
            path: [0; 160],
            path_len: 0,
            https_mode: false,
            tls_handshake_done: false,
            https_handshake_queued: false,
            tls_live: false,
            tls_tcp_eof: false,
            tls_tx_flush_pending: false,
            tls_cipher_in: [0; TLS_CIPHER_RX_CAP],
            tls_cipher_off: 0,
            tls_cipher_len: 0,
            tls_tx_buf: [0; TLS_TX_CAP],
            tls_tx_len: 0,
            tls_poll_vio: core::ptr::null_mut(),
            tls_poll_mac: [0; 6],
            tls_poll_scratch: core::ptr::null_mut(),
            tls_poll_scratch_len: 0,
            tls_rbuf: [0; 16384],
            tls_wbuf: [0; 16384],
            tls: MaybeUninit::uninit(),
            tls_server_name: [0; 96],
            tls_server_name_len: 0,
            stream: [0; STREAM_CAP],
            stream_len: 0,
            header_found: false,
            page_truncated: false,
        }
    }

    pub fn seed_from_mac(&mut self, mac: &[u8; 6]) {
        let mut s = 0x1234_5678u32;
        for b in mac.iter() {
            s = s.wrapping_mul(0x0100_0193).wrapping_add(u32::from(*b));
        }
        self.tcp_seq = s;
    }

    /// Abort in-flight fetch and clear page (keeps gateway ARP).
    pub fn reset_demo(&mut self) {
        self.clear_fetch_inner();
        self.phase = NetPhase::Off;
    }

    fn clear_fetch_inner(&mut self) {
        if self.tls_live {
            unsafe {
                core::ptr::drop_in_place(self.tls.as_mut_ptr());
            }
            self.tls_live = false;
        }
        self.tls_handshake_done = false;
        self.https_handshake_queued = false;
        self.https_mode = false;
        self.tls_cipher_off = 0;
        self.tls_cipher_len = 0;
        self.tls_tx_len = 0;
        self.tls_tcp_eof = false;
        self.tls_tx_flush_pending = false;
        self.tls_server_name_len = 0;
        self.clear_tls_poll();

        self.fetch_armed = false;
        self.fetch_done = false;
        self.syn_sent = false;
        self.get_sent = false;
        self.syn_retries = 0;
        self.http_bytes = 0;
        self.stream_len = 0;
        self.header_found = false;
        self.page_len = 0;
        self.page.fill(0);
        self.stream.fill(0);
        self.fetch_err_len = 0;
        self.fetch_err.fill(0);
        self.needs_dns = false;
        self.dns_done = false;
        self.dns_xmit_phase = 0;
        self.page_truncated = false;
    }

    fn reset_tcp_for_new_fetch(&mut self) {
        self.syn_sent = false;
        self.get_sent = false;
        self.syn_retries = 0;
        self.http_bytes = 0;
        self.stream_len = 0;
        self.header_found = false;
        self.page_len = 0;
        self.page.fill(0);
        self.stream.fill(0);
        self.fetch_done = false;
        self.fetch_err_len = 0;
        self.fetch_err.fill(0);
        self.page_truncated = false;
    }

    /// Parse `url` and start HTTP or HTTPS fetch. Errors copy a short message into `fetch_err`.
    pub fn start_fetch(&mut self, url: &[u8]) {
        self.clear_fetch_inner();
        let Some(p) = parse_fetch_url(url) else {
            self.set_err(b"BAD URL");
            return;
        };
        self.https_mode = p.https;
        self.tls_server_name_len = p.host_for_dns_len;
        self.tls_server_name[..p.host_for_dns_len]
            .copy_from_slice(&p.host_for_dns[..p.host_for_dns_len]);
        self.remote_port = p.port;
        self.host_header_len = p.host_header_len;
        self.host_header[..p.host_header_len].copy_from_slice(&p.host_header[..p.host_header_len]);
        self.path_len = p.path_len;
        self.path[..p.path_len].copy_from_slice(&p.path[..p.path_len]);

        if p.needs_dns {
            self.needs_dns = true;
            self.dns_done = false;
            self.dns_name_len = p.host_for_dns_len;
            self.dns_name[..p.host_for_dns_len]
                .copy_from_slice(&p.host_for_dns[..p.host_for_dns_len]);
            self.dns_tx_id = self.tick as u16 ^ 0xACE1;
            if self.dns_tx_id == 0 {
                self.dns_tx_id = 0xB00F;
            }
        } else {
            self.needs_dns = false;
            self.dns_done = true;
            self.remote_ip = p.ip;
        }

        self.reset_tcp_for_new_fetch();
        self.fetch_armed = true;
        self.page_gen = self.page_gen.wrapping_add(1);
    }

    fn set_err(&mut self, msg: &[u8]) {
        let n = msg.len().min(self.fetch_err.len());
        self.fetch_err[..n].copy_from_slice(&msg[..n]);
        self.fetch_err_len = n;
        self.fetch_armed = false;
        self.fetch_done = true;
        self.page_gen = self.page_gen.wrapping_add(1);
    }

    pub fn drive(&mut self, vio: &mut VirtioNet, our_mac: &[u8; 6], scratch: &mut [u8]) {
        self.tick = self.tick.wrapping_add(1);

        let mut rxb = [0u8; 2048];
        while let Some(n) = unsafe { vio.poll_rx_packet(&mut rxb) } {
            self.handle_rx(&rxb[..n], our_mac, scratch, vio);
        }

        if !self.gw_known {
            self.phase = NetPhase::Arp;
        } else if self.fetch_armed && self.needs_dns && !self.dns_done {
            self.phase = NetPhase::Dns;
        } else if self.fetch_armed && self.dns_done && !self.fetch_done {
            if !self.get_sent {
                self.phase = NetPhase::Tcp;
            } else {
                self.phase = NetPhase::Http;
            }
        } else if self.fetch_done {
            self.phase = NetPhase::Done;
        } else {
            self.phase = NetPhase::Off;
        }

        if !self.gw_known {
            if self.tick % 72 == 0 {
                let len = build_arp_request(our_mac, scratch);
                if len > 0 {
                    unsafe {
                        let _ = vio.transmit(&scratch[..len]);
                    }
                }
            }
            return;
        }

        if self.fetch_armed && self.needs_dns && !self.dns_done {
            self.dns_xmit_phase = self.dns_xmit_phase.wrapping_add(1);
            if self.dns_xmit_phase % 64 == 0 {
                let n = build_dns_udp_packet(
                    our_mac,
                    &self.gw_mac,
                    DNS_LOCAL_PORT,
                    53,
                    &self.dns_name[..self.dns_name_len],
                    self.dns_tx_id,
                    self.ip_id.wrapping_add(1),
                    scratch,
                );
                self.ip_id = self.ip_id.wrapping_add(1);
                if n > 0 {
                    unsafe {
                        let _ = vio.transmit(&scratch[..n]);
                    }
                }
            }
            if self.dns_xmit_phase > 64 * 48 {
                self.set_err(b"DNS TIMEOUT");
            }
            return;
        }

        if self.fetch_armed && self.dns_done && !self.fetch_done {
            if self.syn_retries >= 12 && !self.get_sent {
                self.set_err(b"TCP NO CONNECT");
                return;
            }
            if !self.syn_sent || (self.syn_retries < 12 && self.tick % 96 == 0 && !self.get_sent) {
                let len = build_tcp_syn(
                    our_mac,
                    &self.gw_mac,
                    self.tcp_seq,
                    self.remote_ip,
                    self.remote_port,
                    scratch,
                );
                if len > 0 && unsafe { vio.transmit(&scratch[..len]) } {
                    self.syn_sent = true;
                    self.syn_retries = self.syn_retries.saturating_add(1);
                }
            }

            if self.https_mode
                && self.https_handshake_queued
                && !self.tls_handshake_done
                && !self.fetch_done
            {
                self.https_handshake_queued = false;
                self.run_https_handshake_and_get(vio, our_mac, scratch);
            } else if self.https_mode
                && self.tls_handshake_done
                && self.get_sent
                && !self.fetch_done
            {
                self.tls_pump_application(vio, our_mac, scratch);
            }
        }
    }

    fn handle_rx(
        &mut self,
        frame: &[u8],
        our_mac: &[u8; 6],
        scratch: &mut [u8],
        vio: &mut VirtioNet,
    ) {
        if frame.len() < 14 {
            return;
        }
        let et = u16::from_be_bytes([frame[12], frame[13]]);
        if et == ETH_P_ARP {
            self.handle_arp(frame);
            return;
        }
        if et != ETH_P_IP || frame.len() < 34 {
            return;
        }
        if frame[14] >> 4 != 4 {
            return;
        }
        let ihl = (frame[14] & 0x0f) as usize * 4;
        if frame.len() < 14 + ihl {
            return;
        }
        let dip = [frame[30], frame[31], frame[32], frame[33]];
        if dip != OUR_IP {
            return;
        }
        let sip = [frame[26], frame[27], frame[28], frame[29]];
        let proto = frame[23];

        match proto {
            17 => self.handle_udp_rx(frame, sip, ihl, our_mac, scratch, vio),
            6 => self.handle_tcp_rx(frame, sip, ihl, our_mac, scratch, vio),
            _ => {}
        }
    }

    fn handle_udp_rx(
        &mut self,
        frame: &[u8],
        sip: [u8; 4],
        ihl: usize,
        _our_mac: &[u8; 6],
        _scratch: &mut [u8],
        _vio: &mut VirtioNet,
    ) {
        if sip != DNS_SERVER || !self.fetch_armed || !self.needs_dns || self.dns_done {
            return;
        }
        let u0 = 14 + ihl;
        if frame.len() < u0 + 8 {
            return;
        }
        let dport = u16::from_be_bytes([frame[u0 + 2], frame[u0 + 3]]);
        if dport != DNS_LOCAL_PORT {
            return;
        }
        let udp_len = u16::from_be_bytes([frame[u0 + 4], frame[u0 + 5]]) as usize;
        if udp_len < 8 || u0 + udp_len > frame.len() {
            return;
        }
        let dns = &frame[u0 + 8..u0 + udp_len];
        if let Some(ip) = parse_dns_a(dns, self.dns_tx_id) {
            self.remote_ip = ip;
            self.dns_done = true;
            self.needs_dns = false;
            self.dns_xmit_phase = 0;
            self.syn_sent = false;
            self.syn_retries = 0;
        }
    }

    fn handle_tcp_rx(
        &mut self,
        frame: &[u8],
        sip: [u8; 4],
        ihl: usize,
        our_mac: &[u8; 6],
        scratch: &mut [u8],
        vio: &mut VirtioNet,
    ) {
        let tcp_off = 14 + ihl;
        if frame.len() < tcp_off + 20 {
            return;
        }
        let sport = u16::from_be_bytes([frame[tcp_off], frame[tcp_off + 1]]);
        let dport = u16::from_be_bytes([frame[tcp_off + 2], frame[tcp_off + 3]]);
        if dport != LOCAL_PORT {
            return;
        }
        if sip != self.remote_ip || sport != self.remote_port {
            return;
        }
        if !self.fetch_armed || !self.dns_done {
            return;
        }

        let seq = u32::from_be_bytes([
            frame[tcp_off + 4],
            frame[tcp_off + 5],
            frame[tcp_off + 6],
            frame[tcp_off + 7],
        ]);
        let flg = frame[tcp_off + 13];
        let hlen = ((frame[tcp_off + 12] >> 4) & 0x0f) as usize * 4;
        if frame.len() < tcp_off + hlen {
            return;
        }

        if (flg & 0x04) != 0 {
            self.set_err(b"TCP RST");
            return;
        }

        let payload_off = tcp_off + hlen;
        let payload_len = frame.len().saturating_sub(payload_off);
        let fin = (flg & 0x01) != 0;

        if (flg & 0x12) == 0x12 && !self.get_sent {
            self.tcp_ack = seq.wrapping_add(1);
            self.tcp_seq = self.tcp_seq.wrapping_add(1);
            if self.https_mode {
                let len = build_tcp_ack_only(
                    our_mac,
                    &self.gw_mac,
                    sip,
                    LOCAL_PORT,
                    self.remote_port,
                    self.tcp_seq,
                    self.tcp_ack,
                    self.ip_id.wrapping_add(1),
                    scratch,
                );
                self.ip_id = self.ip_id.wrapping_add(1);
                if len > 0 {
                    unsafe {
                        let _ = vio.transmit(&scratch[..len]);
                    }
                }
                if !self.tls_handshake_done {
                    self.https_handshake_queued = true;
                }
                return;
            }
            let mut pay = [0u8; 384];
            let Some(plen) = build_http_get(
                &self.path[..self.path_len],
                &self.host_header[..self.host_header_len],
                &mut pay,
            ) else {
                self.set_err(b"GET TOO LONG");
                return;
            };
            let len = build_tcp_ack_psh(
                our_mac,
                &self.gw_mac,
                sip,
                LOCAL_PORT,
                self.remote_port,
                self.tcp_seq,
                self.tcp_ack,
                &pay[..plen],
                self.ip_id.wrapping_add(1),
                scratch,
            );
            self.ip_id = self.ip_id.wrapping_add(1);
            if len > 0 && unsafe { vio.transmit(&scratch[..len]) } {
                self.get_sent = true;
                self.tcp_seq = self.tcp_seq.wrapping_add(plen as u32);
            }
            return;
        }

        if payload_len > 0 {
            self.http_bytes = self.http_bytes.wrapping_add(payload_len as u32);
            let pay = &frame[payload_off..payload_off + payload_len];
            if self.https_mode {
                if !self.tls_cipher_append(pay) {
                    self.set_err(b"TLS RX OVFL");
                    return;
                }
            } else {
                self.ingest_tcp_payload(pay);
            }

            let mut ack_seq = seq.wrapping_add(payload_len as u32);
            if fin {
                ack_seq = ack_seq.wrapping_add(1);
            }
            self.tcp_ack = ack_seq;

            let len = build_tcp_ack_only(
                our_mac,
                &self.gw_mac,
                sip,
                LOCAL_PORT,
                self.remote_port,
                self.tcp_seq,
                self.tcp_ack,
                self.ip_id.wrapping_add(1),
                scratch,
            );
            self.ip_id = self.ip_id.wrapping_add(1);
            if len > 0 {
                unsafe {
                    let _ = vio.transmit(&scratch[..len]);
                }
            }

            if fin {
                if self.https_mode {
                    self.tls_tcp_eof = true;
                } else {
                    self.finish_fetch();
                }
            }
            return;
        }

        if fin && self.get_sent {
            self.tcp_ack = seq.wrapping_add(1);
            let len = build_tcp_ack_only(
                our_mac,
                &self.gw_mac,
                sip,
                LOCAL_PORT,
                self.remote_port,
                self.tcp_seq,
                self.tcp_ack,
                self.ip_id.wrapping_add(1),
                scratch,
            );
            self.ip_id = self.ip_id.wrapping_add(1);
            if len > 0 {
                unsafe {
                    let _ = vio.transmit(&scratch[..len]);
                }
            }
            if self.https_mode {
                self.tls_tcp_eof = true;
            } else {
                self.finish_fetch();
            }
        }
    }

    fn clear_tls_poll(&mut self) {
        self.tls_poll_vio = core::ptr::null_mut();
        self.tls_poll_scratch = core::ptr::null_mut();
        self.tls_poll_scratch_len = 0;
    }

    fn set_tls_poll(&mut self, vio: *mut VirtioNet, mac: &[u8; 6], scratch: &mut [u8]) {
        self.tls_poll_vio = vio;
        self.tls_poll_mac.copy_from_slice(mac);
        self.tls_poll_scratch = scratch.as_mut_ptr();
        self.tls_poll_scratch_len = scratch.len();
    }

    fn tls_cipher_compact(&mut self) {
        if self.tls_cipher_off == 0 {
            return;
        }
        let rem = self.tls_cipher_len.saturating_sub(self.tls_cipher_off);
        if rem > 0 {
            self.tls_cipher_in
                .copy_within(self.tls_cipher_off..self.tls_cipher_len, 0);
        }
        self.tls_cipher_len = rem;
        self.tls_cipher_off = 0;
    }

    fn tls_cipher_append(&mut self, data: &[u8]) -> bool {
        if self.tls_cipher_len.saturating_add(data.len()) > self.tls_cipher_in.len() {
            self.tls_cipher_compact();
        }
        if self.tls_cipher_len.saturating_add(data.len()) > self.tls_cipher_in.len() {
            return false;
        }
        let end = self.tls_cipher_len;
        self.tls_cipher_in[end..end + data.len()].copy_from_slice(data);
        self.tls_cipher_len += data.len();
        true
    }

    pub(crate) fn tls_cipher_pop(&mut self, buf: &mut [u8]) -> usize {
        let avail = self.tls_cipher_len.saturating_sub(self.tls_cipher_off);
        if avail == 0 {
            return 0;
        }
        let n = avail.min(buf.len());
        buf[..n].copy_from_slice(&self.tls_cipher_in[self.tls_cipher_off..self.tls_cipher_off + n]);
        self.tls_cipher_off += n;
        if self.tls_cipher_off >= self.tls_cipher_len {
            self.tls_cipher_off = 0;
            self.tls_cipher_len = 0;
        } else if self.tls_cipher_off > 24576 {
            self.tls_cipher_compact();
        }
        n
    }

    pub(crate) fn tls_cipher_tx_append(&mut self, data: &[u8]) -> bool {
        if self.tls_tx_len.saturating_add(data.len()) > self.tls_tx_buf.len() {
            return false;
        }
        let e = self.tls_tx_len;
        self.tls_tx_buf[e..e + data.len()].copy_from_slice(data);
        self.tls_tx_len += data.len();
        true
    }

    fn tls_tx_flush_all(&mut self, vio: &mut VirtioNet, our_mac: &[u8; 6], scratch: &mut [u8]) {
        const MSS: usize = 1400;
        while self.tls_tx_len > 0 {
            let n = self.tls_tx_len.min(MSS);
            let len = build_tcp_ack_psh(
                our_mac,
                &self.gw_mac,
                self.remote_ip,
                LOCAL_PORT,
                self.remote_port,
                self.tcp_seq,
                self.tcp_ack,
                &self.tls_tx_buf[..n],
                self.ip_id.wrapping_add(1),
                scratch,
            );
            self.ip_id = self.ip_id.wrapping_add(1);
            if len == 0 || !unsafe { vio.transmit(&scratch[..len]) } {
                break;
            }
            self.tcp_seq = self.tcp_seq.wrapping_add(n as u32);
            self.tls_tx_buf.copy_within(n..self.tls_tx_len, 0);
            self.tls_tx_len -= n;
        }
        self.tls_tx_flush_pending = false;
    }

    pub(crate) fn tls_spin_poll(&mut self) {
        if self.tls_poll_vio.is_null() {
            return;
        }
        let poll_mac = self.tls_poll_mac;
        unsafe {
            let vio = &mut *self.tls_poll_vio;
            let scratch = core::slice::from_raw_parts_mut(
                self.tls_poll_scratch,
                self.tls_poll_scratch_len,
            );
            self.tls_tx_flush_all(vio, &poll_mac, scratch);
            let mut rxb = [0u8; 2048];
            while let Some(n) = vio.poll_rx_packet(&mut rxb) {
                self.handle_rx(&rxb[..n], &poll_mac, scratch, vio);
            }
        }
    }

    pub(crate) fn tls_eof_from_tcp(&self) -> bool {
        self.tls_tcp_eof
    }

    pub(crate) fn tls_note_flush_pending(&mut self) {
        self.tls_tx_flush_pending = true;
    }

    fn run_https_handshake_and_get(&mut self, vio: &mut VirtioNet, our_mac: &[u8; 6], scratch: &mut [u8]) {
        self.set_tls_poll(vio as *mut VirtioNet, our_mac, scratch);
        let mut sn = [0u8; 96];
        let nl = self.tls_server_name_len;
        if nl > sn.len() {
            self.clear_tls_poll();
            self.set_err(b"TLS BAD HOST");
            return;
        }
        sn[..nl].copy_from_slice(&self.tls_server_name[..nl]);
        let host = match core::str::from_utf8(&sn[..nl]) {
            Ok(s) => s,
            Err(_) => {
                self.clear_tls_poll();
                self.set_err(b"TLS BAD HOST");
                return;
            }
        };
        let config = TlsConfig::new().with_server_name(host);
        let bridge = TlsNetBridge {
            net: self as *mut NetStack,
        };
        let tls_r = core::ptr::addr_of_mut!(self.tls_rbuf);
        let tls_w = core::ptr::addr_of_mut!(self.tls_wbuf);
        let mut tls = unsafe { TlsConnection::new(bridge, &mut *tls_r, &mut *tls_w) };
        let seed = u64::from(self.tcp_seq) ^ u64::from(self.tick);
        let rng = EveRng::new(seed);
        let prov = UnsecureProvider::new::<Aes128GcmSha256>(rng);
        if tls.open(TlsContext::new(&config, prov)).is_err() {
            self.clear_tls_poll();
            self.set_err(b"TLS HS FAIL");
            return;
        }
        self.tls.write(tls);
        self.tls_live = true;
        self.tls_handshake_done = true;

        let mut pay = [0u8; 384];
        let Some(plen) = build_http_get(
            &self.path[..self.path_len],
            &self.host_header[..self.host_header_len],
            &mut pay,
        ) else {
            unsafe {
                core::ptr::drop_in_place(self.tls.as_mut_ptr());
            }
            self.tls_live = false;
            self.tls_handshake_done = false;
            self.clear_tls_poll();
            self.set_err(b"GET TOO LONG");
            return;
        };
        let w_ok = {
            let t = unsafe { self.tls.assume_init_mut() };
            t.write_all(&pay[..plen]).is_ok() && t.flush().is_ok()
        };
        if !w_ok {
            unsafe {
                core::ptr::drop_in_place(self.tls.as_mut_ptr());
            }
            self.tls_live = false;
            self.tls_handshake_done = false;
            self.clear_tls_poll();
            self.set_err(b"TLS WRITE FAIL");
            return;
        }
        self.tls_tx_flush_all(vio, our_mac, scratch);
        self.get_sent = true;
        self.clear_tls_poll();
    }

    fn tls_pump_application(&mut self, vio: &mut VirtioNet, our_mac: &[u8; 6], scratch: &mut [u8]) {
        if !self.tls_live {
            return;
        }
        self.set_tls_poll(vio as *mut VirtioNet, our_mac, scratch);
        self.tls_spin_poll();
        let mut tmp = [0u8; 2048];
        loop {
            let n = unsafe {
                match self.tls.assume_init_mut().read(&mut tmp) {
                    Ok(n) => n,
                    Err(_) => {
                        self.set_err(b"TLS READ ERR");
                        break;
                    }
                }
            };
            if n == 0 {
                break;
            }
            self.ingest_tcp_payload(&tmp[..n]);
        }
        self.tls_tx_flush_all(vio, our_mac, scratch);
        self.clear_tls_poll();
        if self.tls_tcp_eof && !self.fetch_done {
            self.finish_fetch();
        }
    }

    fn finish_fetch(&mut self) {
        if self.fetch_done {
            return;
        }
        self.fetch_done = true;
        self.fetch_armed = false;
        self.page_gen = self.page_gen.wrapping_add(1);
    }

    fn ingest_tcp_payload(&mut self, data: &[u8]) {
        if !self.header_found {
            let room = self.stream.len().saturating_sub(self.stream_len);
            let n = data.len().min(room);
            self.stream[self.stream_len..self.stream_len + n].copy_from_slice(&data[..n]);
            self.stream_len += n;
            if self.stream_len >= self.stream.len() && find_crlfcrlf(&self.stream[..self.stream_len]).is_none()
            {
                self.set_err(b"HTTP HDR TOO BIG");
                return;
            }
            if let Some(pos) = find_crlfcrlf(&self.stream[..self.stream_len]) {
                self.header_found = true;
                let body_off = pos + 4;
                let end = self.stream_len;
                for i in body_off..end {
                    self.page_push_byte(self.stream[i]);
                }
                self.stream_len = 0;
                self.page_gen = self.page_gen.wrapping_add(1);
            }
        } else {
            self.append_to_page(data);
        }
    }

    fn page_push_byte(&mut self, b: u8) {
        if self.page_len < self.page.len() {
            self.page[self.page_len] = b;
            self.page_len += 1;
        } else {
            self.page_truncated = true;
        }
    }

    fn append_to_page(&mut self, data: &[u8]) {
        let room = self.page.len().saturating_sub(self.page_len);
        let n = data.len().min(room);
        if n > 0 {
            self.page[self.page_len..self.page_len + n].copy_from_slice(&data[..n]);
            self.page_len += n;
        }
        if data.len() > n {
            self.page_truncated = true;
        }
    }

    fn handle_arp(&mut self, frame: &[u8]) {
        if frame.len() < 14 + 28 {
            return;
        }
        let a = 14;
        let op = u16::from_be_bytes([frame[a + 6], frame[a + 7]]);
        if op != ARP_OP_REPLY {
            return;
        }
        let tpa = [frame[a + 24], frame[a + 25], frame[a + 26], frame[a + 27]];
        if tpa != OUR_IP {
            return;
        }
        let spa = [frame[a + 14], frame[a + 15], frame[a + 16], frame[a + 17]];
        if spa != GW_IP {
            return;
        }
        self.gw_mac.copy_from_slice(&frame[a + 8..a + 14]);
        self.gw_known = true;
    }
}

fn find_crlfcrlf(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn parse_dns_a(buf: &[u8], expected_id: u16) -> Option<[u8; 4]> {
    if buf.len() < 12 {
        return None;
    }
    let id = u16::from_be_bytes([buf[0], buf[1]]);
    if id != expected_id {
        return None;
    }
    let qd = u16::from_be_bytes([buf[4], buf[5]]) as usize;
    let an = u16::from_be_bytes([buf[6], buf[7]]) as usize;
    let mut off = 12usize;
    for _ in 0..qd {
        off = skip_dns_name(buf, off)?;
        if off + 4 > buf.len() {
            return None;
        }
        off += 4;
    }
    for _ in 0..an {
        off = skip_dns_name(buf, off)?;
        if off + 10 > buf.len() {
            return None;
        }
        let typ = u16::from_be_bytes([buf[off], buf[off + 1]]);
        let rdlen = u16::from_be_bytes([buf[off + 8], buf[off + 9]]) as usize;
        off += 10;
        if off + rdlen > buf.len() {
            return None;
        }
        if typ == 1 && rdlen == 4 {
            return Some([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]);
        }
        off += rdlen;
    }
    None
}

fn skip_dns_name(buf: &[u8], mut i: usize) -> Option<usize> {
    loop {
        if i >= buf.len() {
            return None;
        }
        let len = buf[i];
        if len == 0 {
            return Some(i + 1);
        }
        if len & 0xC0 == 0xC0 {
            return Some(i + 2);
        }
        let l = len as usize;
        i = i.checked_add(1)?.checked_add(l)?;
        if i > buf.len() {
            return None;
        }
    }
}

fn build_http_get(path: &[u8], host: &[u8], out: &mut [u8]) -> Option<usize> {
    const P1: &[u8] = b"GET ";
    const P2: &[u8] = b" HTTP/1.0\r\nHost: ";
    const P3: &[u8] = b"\r\nConnection: close\r\n\r\n";
    let need = P1.len() + path.len() + P2.len() + host.len() + P3.len();
    if need > out.len() {
        return None;
    }
    let mut i = 0;
    out[i..i + P1.len()].copy_from_slice(P1);
    i += P1.len();
    out[i..i + path.len()].copy_from_slice(path);
    i += path.len();
    out[i..i + P2.len()].copy_from_slice(P2);
    i += P2.len();
    out[i..i + host.len()].copy_from_slice(host);
    i += host.len();
    out[i..i + P3.len()].copy_from_slice(P3);
    i += P3.len();
    Some(i)
}

fn sum16(mut data: &[u8], mut sum: u32) -> u16 {
    while data.len() >= 2 {
        sum += u16::from_be_bytes([data[0], data[1]]) as u32;
        data = &data[2..];
    }
    if !data.is_empty() {
        sum += (data[0] as u32) << 8;
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

fn build_arp_request(our_mac: &[u8; 6], out: &mut [u8]) -> usize {
    const PKT: usize = 42;
    let total = VIRTIO_NET_HDR + PKT;
    if out.len() < total {
        return 0;
    }
    out[..VIRTIO_NET_HDR].fill(0);
    let o = VIRTIO_NET_HDR;
    out[o..o + 6].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
    out[o + 6..o + 12].copy_from_slice(our_mac);
    out[o + 12..o + 14].copy_from_slice(&ETH_P_ARP.to_be_bytes());
    out[o + 14..o + 16].copy_from_slice(&ARP_HTYPE_ETH.to_be_bytes());
    out[o + 16..o + 18].copy_from_slice(&ARP_PTYPE_IP.to_be_bytes());
    out[o + 18] = 6;
    out[o + 19] = 4;
    out[o + 20..o + 22].copy_from_slice(&ARP_OP_REQ.to_be_bytes());
    out[o + 22..o + 28].copy_from_slice(our_mac);
    out[o + 28..o + 32].copy_from_slice(&OUR_IP);
    out[o + 32..o + 38].fill(0);
    out[o + 38..o + 42].copy_from_slice(&GW_IP);
    total
}

fn build_tcp_syn(
    our_mac: &[u8; 6],
    gw_mac: &[u8; 6],
    seq: u32,
    remote_ip: [u8; 4],
    remote_port: u16,
    out: &mut [u8],
) -> usize {
    let tcp_len = 20usize;
    let ip_len = 20 + tcp_len;
    let eth_len = 14 + ip_len;
    let total = VIRTIO_NET_HDR + eth_len;
    if out.len() < total {
        return 0;
    }
    out[..VIRTIO_NET_HDR].fill(0);
    let e = VIRTIO_NET_HDR;
    out[e..e + 6].copy_from_slice(gw_mac);
    out[e + 6..e + 12].copy_from_slice(our_mac);
    out[e + 12..e + 14].copy_from_slice(&ETH_P_IP.to_be_bytes());

    let ip = e + 14;
    out[ip] = 0x45;
    out[ip + 1] = 0;
    out[ip + 2..ip + 4].copy_from_slice(&(ip_len as u16).to_be_bytes());
    out[ip + 4..ip + 6].copy_from_slice(&0u16.to_be_bytes());
    out[ip + 6..ip + 8].copy_from_slice(&0u16.to_be_bytes());
    out[ip + 8] = 64;
    out[ip + 9] = 6;
    out[ip + 10..ip + 12].copy_from_slice(&0u16.to_be_bytes());
    out[ip + 12..ip + 16].copy_from_slice(&OUR_IP);
    out[ip + 16..ip + 20].copy_from_slice(&remote_ip);
    let csum = sum16(&out[ip..ip + 20], 0);
    out[ip + 10..ip + 12].copy_from_slice(&csum.to_be_bytes());

    let t = ip + 20;
    out[t..t + 2].copy_from_slice(&LOCAL_PORT.to_be_bytes());
    out[t + 2..t + 4].copy_from_slice(&remote_port.to_be_bytes());
    out[t + 4..t + 8].copy_from_slice(&seq.to_be_bytes());
    out[t + 8..t + 12].copy_from_slice(&0u32.to_be_bytes());
    out[t + 12] = 0x50;
    out[t + 13] = 0x02;
    out[t + 14..t + 16].copy_from_slice(&0x2000u16.to_be_bytes());
    out[t + 16..t + 18].copy_from_slice(&0u16.to_be_bytes());
    out[t + 18..t + 20].copy_from_slice(&0u16.to_be_bytes());
    let ph = pseudo_sum(OUR_IP, remote_ip, 6, tcp_len as u16);
    let tc = sum16(&out[t..t + tcp_len], ph as u32);
    out[t + 16..t + 18].copy_from_slice(&tc.to_be_bytes());
    total
}

fn build_tcp_ack_psh(
    our_mac: &[u8; 6],
    gw_mac: &[u8; 6],
    remote_ip: [u8; 4],
    sport: u16,
    dport: u16,
    seq: u32,
    ack: u32,
    payload: &[u8],
    ip_ident: u16,
    out: &mut [u8],
) -> usize {
    let tcp_len = 20 + payload.len();
    let ip_len = 20 + tcp_len;
    let eth_len = 14 + ip_len;
    let total = VIRTIO_NET_HDR + eth_len;
    if out.len() < total {
        return 0;
    }
    out[..VIRTIO_NET_HDR].fill(0);
    let e = VIRTIO_NET_HDR;
    out[e..e + 6].copy_from_slice(gw_mac);
    out[e + 6..e + 12].copy_from_slice(our_mac);
    out[e + 12..e + 14].copy_from_slice(&ETH_P_IP.to_be_bytes());

    let ip = e + 14;
    out[ip] = 0x45;
    out[ip + 1] = 0;
    out[ip + 2..ip + 4].copy_from_slice(&(ip_len as u16).to_be_bytes());
    out[ip + 4..ip + 6].copy_from_slice(&ip_ident.to_be_bytes());
    out[ip + 6..ip + 8].copy_from_slice(&0u16.to_be_bytes());
    out[ip + 8] = 64;
    out[ip + 9] = 6;
    out[ip + 10..ip + 12].copy_from_slice(&0u16.to_be_bytes());
    out[ip + 12..ip + 16].copy_from_slice(&OUR_IP);
    out[ip + 16..ip + 20].copy_from_slice(&remote_ip);
    let csum = sum16(&out[ip..ip + 20], 0);
    out[ip + 10..ip + 12].copy_from_slice(&csum.to_be_bytes());

    let t = ip + 20;
    out[t..t + 2].copy_from_slice(&sport.to_be_bytes());
    out[t + 2..t + 4].copy_from_slice(&dport.to_be_bytes());
    out[t + 4..t + 8].copy_from_slice(&seq.to_be_bytes());
    out[t + 8..t + 12].copy_from_slice(&ack.to_be_bytes());
    out[t + 12] = 0x50;
    out[t + 13] = 0x18;
    out[t + 14..t + 16].copy_from_slice(&0x8000u16.to_be_bytes());
    out[t + 16..t + 18].copy_from_slice(&0u16.to_be_bytes());
    out[t + 18..t + 20].copy_from_slice(&0u16.to_be_bytes());
    out[t + 20..t + 20 + payload.len()].copy_from_slice(payload);
    let ph = pseudo_sum(OUR_IP, remote_ip, 6, tcp_len as u16);
    let tc = sum16(&out[t..t + tcp_len], ph as u32);
    out[t + 16..t + 18].copy_from_slice(&tc.to_be_bytes());
    total
}

fn build_tcp_ack_only(
    our_mac: &[u8; 6],
    gw_mac: &[u8; 6],
    remote_ip: [u8; 4],
    sport: u16,
    dport: u16,
    seq: u32,
    ack: u32,
    ip_ident: u16,
    out: &mut [u8],
) -> usize {
    let tcp_len = 20usize;
    let ip_len = 20 + tcp_len;
    let eth_len = 14 + ip_len;
    let total = VIRTIO_NET_HDR + eth_len;
    if out.len() < total {
        return 0;
    }
    out[..VIRTIO_NET_HDR].fill(0);
    let e = VIRTIO_NET_HDR;
    out[e..e + 6].copy_from_slice(gw_mac);
    out[e + 6..e + 12].copy_from_slice(our_mac);
    out[e + 12..e + 14].copy_from_slice(&ETH_P_IP.to_be_bytes());

    let ip = e + 14;
    out[ip] = 0x45;
    out[ip + 1] = 0;
    out[ip + 2..ip + 4].copy_from_slice(&(ip_len as u16).to_be_bytes());
    out[ip + 4..ip + 6].copy_from_slice(&ip_ident.to_be_bytes());
    out[ip + 6..ip + 8].copy_from_slice(&0u16.to_be_bytes());
    out[ip + 8] = 64;
    out[ip + 9] = 6;
    out[ip + 10..ip + 12].copy_from_slice(&0u16.to_be_bytes());
    out[ip + 12..ip + 16].copy_from_slice(&OUR_IP);
    out[ip + 16..ip + 20].copy_from_slice(&remote_ip);
    let csum = sum16(&out[ip..ip + 20], 0);
    out[ip + 10..ip + 12].copy_from_slice(&csum.to_be_bytes());

    let t = ip + 20;
    out[t..t + 2].copy_from_slice(&sport.to_be_bytes());
    out[t + 2..t + 4].copy_from_slice(&dport.to_be_bytes());
    out[t + 4..t + 8].copy_from_slice(&seq.to_be_bytes());
    out[t + 8..t + 12].copy_from_slice(&ack.to_be_bytes());
    out[t + 12] = 0x50;
    out[t + 13] = 0x10;
    out[t + 14..t + 16].copy_from_slice(&0x8000u16.to_be_bytes());
    out[t + 16..t + 18].copy_from_slice(&0u16.to_be_bytes());
    out[t + 18..t + 20].copy_from_slice(&0u16.to_be_bytes());
    let ph = pseudo_sum(OUR_IP, remote_ip, 6, tcp_len as u16);
    let tc = sum16(&out[t..t + tcp_len], ph as u32);
    out[t + 16..t + 18].copy_from_slice(&tc.to_be_bytes());
    total
}

fn pseudo_sum(src: [u8; 4], dst: [u8; 4], proto: u8, len: u16) -> u32 {
    let mut s = 0u32;
    s += u16::from_be_bytes([src[0], src[1]]) as u32;
    s += u16::from_be_bytes([src[2], src[3]]) as u32;
    s += u16::from_be_bytes([dst[0], dst[1]]) as u32;
    s += u16::from_be_bytes([dst[2], dst[3]]) as u32;
    s += u32::from(proto);
    s += u32::from(len);
    s
}

fn encode_dns_qname(hostname: &[u8], out: &mut [u8]) -> Option<usize> {
    if hostname.is_empty() || hostname.len() > 200 {
        return None;
    }
    let mut pos = 0usize;
    let mut start = 0usize;
    for (i, &c) in hostname.iter().enumerate() {
        if c == b'.' {
            let lab = i - start;
            if lab == 0 || lab > 63 || pos + 1 + lab > out.len() {
                return None;
            }
            out[pos] = lab as u8;
            pos += 1;
            out[pos..pos + lab].copy_from_slice(&hostname[start..i]);
            pos += lab;
            start = i + 1;
        }
    }
    let lab = hostname.len() - start;
    if lab == 0 || lab > 63 || pos + 1 + lab + 1 > out.len() {
        return None;
    }
    out[pos] = lab as u8;
    pos += 1;
    out[pos..pos + lab].copy_from_slice(&hostname[start..]);
    pos += lab;
    out[pos] = 0;
    pos += 1;
    Some(pos)
}

fn build_dns_udp_packet(
    our_mac: &[u8; 6],
    gw_mac: &[u8; 6],
    sport: u16,
    dport: u16,
    hostname: &[u8],
    tx_id: u16,
    ip_ident: u16,
    out: &mut [u8],
) -> usize {
    let mut dns = [0u8; 512];
    let mut dp = 0usize;
    dns[dp..dp + 2].copy_from_slice(&tx_id.to_be_bytes());
    dp += 2;
    dns[dp..dp + 2].copy_from_slice(&0x0100u16.to_be_bytes());
    dp += 2;
    dns[dp..dp + 2].copy_from_slice(&1u16.to_be_bytes());
    dp += 2;
    dns[dp..dp + 6].fill(0);
    dp += 6;
    let Some(nq) = encode_dns_qname(hostname, &mut dns[dp..]) else {
        return 0;
    };
    dp += nq;
    dns[dp..dp + 2].copy_from_slice(&1u16.to_be_bytes());
    dp += 2;
    dns[dp..dp + 2].copy_from_slice(&1u16.to_be_bytes());
    dp += 2;

    let udp_len = 8 + dp;
    let ip_len = 20 + udp_len;
    let eth_len = 14 + ip_len;
    let total = VIRTIO_NET_HDR + eth_len;
    if out.len() < total {
        return 0;
    }
    out[..VIRTIO_NET_HDR].fill(0);
    let e = VIRTIO_NET_HDR;
    out[e..e + 6].copy_from_slice(gw_mac);
    out[e + 6..e + 12].copy_from_slice(our_mac);
    out[e + 12..e + 14].copy_from_slice(&ETH_P_IP.to_be_bytes());

    let ip = e + 14;
    out[ip] = 0x45;
    out[ip + 1] = 0;
    out[ip + 2..ip + 4].copy_from_slice(&(ip_len as u16).to_be_bytes());
    out[ip + 4..ip + 6].copy_from_slice(&ip_ident.to_be_bytes());
    out[ip + 6..ip + 8].copy_from_slice(&0u16.to_be_bytes());
    out[ip + 8] = 64;
    out[ip + 9] = 17;
    out[ip + 10..ip + 12].copy_from_slice(&0u16.to_be_bytes());
    out[ip + 12..ip + 16].copy_from_slice(&OUR_IP);
    out[ip + 16..ip + 20].copy_from_slice(&DNS_SERVER);
    let csum = sum16(&out[ip..ip + 20], 0);
    out[ip + 10..ip + 12].copy_from_slice(&csum.to_be_bytes());

    let u = ip + 20;
    out[u..u + 2].copy_from_slice(&sport.to_be_bytes());
    out[u + 2..u + 4].copy_from_slice(&dport.to_be_bytes());
    out[u + 4..u + 6].copy_from_slice(&(udp_len as u16).to_be_bytes());
    out[u + 6..u + 8].copy_from_slice(&0u16.to_be_bytes());
    out[u + 8..u + 8 + dp].copy_from_slice(&dns[..dp]);

    let ph = pseudo_sum(OUR_IP, DNS_SERVER, 17, udp_len as u16);
    let ucsum = sum16(&out[u..u + udp_len], ph as u32);
    out[u + 6..u + 8].copy_from_slice(&ucsum.to_be_bytes());

    total
}
