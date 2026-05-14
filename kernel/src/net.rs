// SPDX-License-Identifier: MIT OR Apache-2.0

//! ARP, DNS (UDP), and minimal TCP/HTTP/1.0 client. Addresses come from **SLIRP defaults**,
//! **DHCP**, or **static** SYS settings (`DeviceSettings` / `NetIpv4Addrs`).
//! **DHCP + QEMU user NAT:** if DHCP does not complete in time (some UTM/QEMU paths), the stack
//! falls back to **SLIRP** (`10.0.2.15` / `.2` / `.3`) so HTTP/HTTPS still works — same “keep the
//! guest shell useful with a small stack” idea as TempleOS-family OSes (e.g. [ZealOS](https://github.com/Zeal-Operating-System/ZealOS), Unlicense), without vendoring HolyC code.
//! **`https://`** uses TLS 1.3 via `embedded-tls` with certificate + hostname verification.
//! Wall time for X.509 validity is approximate (build epoch + guest uptime ticks); see
//! [`crate::eve_tls::wall_clock_note_net_tick`].
//!
//! **QEMU user NAT:** default SLIRP triple is `10.0.2.15` / `10.0.2.2` / `10.0.2.3`.

use core::mem::MaybeUninit;

use crate::diag_log;
use crate::eve_tls::{
    DIGICERT_GLOBAL_ROOT_G2_DER, ISRG_ROOT_X1_DER, EveVerifiedTlsProvider, TlsNetBridge,
};
use crate::net_ipv4::NetIpv4Addrs;
use crate::nic::AnyNic;
use crate::settings::{DeviceSettings, IpConfig};
use crate::url::parse_fetch_url;
use embedded_io::Write as _;
use embedded_tls::blocking::{Aes128GcmSha256, Certificate, TlsConfig, TlsConnection, TlsContext};
use brotli_decompressor::{brotli_decode_prealloc, BrotliResult, HuffmanCode};
use miniz_oxide::inflate::{decompress_slice_iter_to_slice, TINFLStatus};

pub const VIRTIO_NET_HDR: usize = 12;

const TLS_CIPHER_RX_CAP: usize = 49152;
const TLS_TX_CAP: usize = 24576;

const LOCAL_PORT: u16 = 49152;
const DNS_LOCAL_PORT: u16 = 53000;
const DHCP_CLIENT_PORT: u16 = 68;
const DHCP_SERVER_PORT: u16 = 67;
const DHCP_MAGIC: [u8; 4] = [99, 130, 83, 99];
const OPT_ROUTER: u8 = 3;
const OPT_DNS: u8 = 6;
const OPT_REQUESTED_IP: u8 = 50;
const OPT_MSG_TYPE: u8 = 53;
const OPT_SERVER_ID: u8 = 54;
const OPT_END: u8 = 255;
const DHCP_DISCOVER: u8 = 1;
const DHCP_OFFER: u8 = 2;
const DHCP_REQUEST: u8 = 3;
const DHCP_ACK: u8 = 5;

/// One `Content-Encoding` coding in **header order** (first listed = applied first to the
/// payload; decode in reverse). See RFC 9110 §8.4.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BodyLayer {
    Identity,
    Gzip,
    /// HTTP `deflate` = zlib wrapper (RFC 7230); raw DEFLATE is not accepted here.
    DeflateZlib,
    Brotli,
}

/// Scratch for [`brotli_decode_prealloc`] (sizes match `brotli-decompressor` unit-test stack pools).
const BROTLI_SCRATCH_U8: usize = 300 * 1024;
const BROTLI_SCRATCH_U32: usize = 12 * 1024;
const BROTLI_SCRATCH_HUFFMAN: usize = 18 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
enum DhcpPhase {
    Idle,
    WaitOffer,
    WaitAck,
    Bound,
}

/// Max bytes stored for one HTTP response body (compressed or plain). Gzip members larger than
/// this truncate (`page_truncated`); decompressed output is capped separately by [`GUNZIP_BUF_CAP`].
const PAGE_CAP: usize = 65536;
/// Upper bound for gunzip output (plain HTML/bytes copied back into [`NetStack::page`]).
const GUNZIP_BUF_CAP: usize = 65536;
const STREAM_CAP: usize = 4096;
/// VirtIO / TLS RX staging (kept on `NetStack`, not the kernel stack — avoids nested 2 KiB frames).
const NIC_RX_IOBUF: usize = 2048;

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
    /// Guest / gateway / DNS for this stack.
    pub addrs: NetIpv4Addrs,
    ip_settings_tag: u32,
    dhcp_phase: DhcpPhase,
    dhcp_xid: u32,
    dhcp_server_id: [u8; 4],
    dhcp_offer_yi: [u8; 4],
    dhcp_phase_start_tick: u32,
    dhcp_last_tx_tick: u32,
    gw_mac: [u8; 6],
    gw_known: bool,
    tick: u32,
    ip_id: u16,
    tcp_seq: u32,
    tcp_ack: u32,
    syn_sent: bool,
    get_sent: bool,
    syn_retries: u8,
    fetch_progress_tick: u32,
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

    tls_poll_nic: *mut AnyNic,
    tls_poll_mac: [u8; 6],
    tls_poll_scratch: *mut u8,
    tls_poll_scratch_len: usize,

    tls_rbuf: [u8; 16384],
    tls_wbuf: [u8; 16384],
    tls: MaybeUninit<TlsConnection<'static, TlsNetBridge, Aes128GcmSha256>>,
    tls_server_name: [u8; 96],
    tls_server_name_len: usize,

    drive_rx_buf: [u8; NIC_RX_IOBUF],
    tls_spin_rx_buf: [u8; NIC_RX_IOBUF],
    tls_read_tmp: [u8; NIC_RX_IOBUF],

    stream: [u8; STREAM_CAP],
    stream_len: usize,
    header_found: bool,
    /// Plain HTTP (and cleartext bytes after TLS decrypt): body bytes still needed when
    /// `Content-Length` was present. `None` = no CL (or chunked): complete on TCP FIN only.
    http_body_remaining: Option<usize>,
    /// First TCP SYN for this fetch; used to time out connect when SYN-ACK never arrives.
    tcp_connect_start_tick: u32,
    pub page_truncated: bool,
    /// Codings in **header list order** (application order); at most four layers.
    body_layers: [BodyLayer; 4],
    body_layer_count: u8,
    gunzip_buf: [u8; GUNZIP_BUF_CAP],
    brotli_scratch_u8: [u8; BROTLI_SCRATCH_U8],
    brotli_scratch_u32: [u32; BROTLI_SCRATCH_U32],
    brotli_scratch_huffman: [HuffmanCode; BROTLI_SCRATCH_HUFFMAN],
}

impl NetStack {
    /// Fresh stack state. Prefer [`Self::STATIC_INITIAL`] + `static mut` for the singleton so the
    /// ~750 KiB struct is not materialized on the kernel stack during boot (avoids triple-fault
    /// reboot loops when the stack margin is tight — e.g. some emulated UTM/QEMU paths).
    pub const fn static_initial() -> Self {
        Self {
            addrs: NetIpv4Addrs::SLIRP,
            ip_settings_tag: u32::MAX,
            dhcp_phase: DhcpPhase::Idle,
            dhcp_xid: 0,
            dhcp_server_id: [0; 4],
            dhcp_offer_yi: [0; 4],
            dhcp_phase_start_tick: 0,
            dhcp_last_tx_tick: 0,
            gw_mac: [0; 6],
            gw_known: false,
            tick: 0,
            ip_id: 0x4000,
            tcp_seq: 0,
            tcp_ack: 0,
            syn_sent: false,
            get_sent: false,
            syn_retries: 0,
            fetch_progress_tick: 0,
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
            tls_poll_nic: core::ptr::null_mut(),
            tls_poll_mac: [0; 6],
            tls_poll_scratch: core::ptr::null_mut(),
            tls_poll_scratch_len: 0,
            tls_rbuf: [0; 16384],
            tls_wbuf: [0; 16384],
            tls: MaybeUninit::uninit(),
            tls_server_name: [0; 96],
            tls_server_name_len: 0,
            drive_rx_buf: [0; NIC_RX_IOBUF],
            tls_spin_rx_buf: [0; NIC_RX_IOBUF],
            tls_read_tmp: [0; NIC_RX_IOBUF],
            stream: [0; STREAM_CAP],
            stream_len: 0,
            header_found: false,
            http_body_remaining: None,
            tcp_connect_start_tick: 0,
            page_truncated: false,
            body_layers: [BodyLayer::Identity; 4],
            body_layer_count: 0,
            gunzip_buf: [0; GUNZIP_BUF_CAP],
            brotli_scratch_u8: [0; BROTLI_SCRATCH_U8],
            brotli_scratch_u32: [0; BROTLI_SCRATCH_U32],
            brotli_scratch_huffman: [HuffmanCode { value: 0, bits: 0 }; BROTLI_SCRATCH_HUFFMAN],
        }
    }

    pub fn seed_from_mac(&mut self, mac: &[u8; 6]) {
        let mut s = 0x1234_5678u32;
        for b in mac.iter() {
            s = s.wrapping_mul(0x0100_0193).wrapping_add(u32::from(*b));
        }
        self.tcp_seq = s;
    }

    pub fn sync_ip_from_settings(&mut self, s: &DeviceSettings, tag: u32) {
        if tag == self.ip_settings_tag {
            return;
        }
        self.ip_settings_tag = tag;
        self.gw_known = false;
        self.dhcp_phase_start_tick = 0;
        self.dhcp_last_tx_tick = 0;
        match s.ip_config {
            IpConfig::Slirp => {
                diag_log::line(b"net ipcfg slirp");
                self.addrs = NetIpv4Addrs::SLIRP;
                self.dhcp_phase = DhcpPhase::Idle;
            }
            IpConfig::Static => {
                diag_log::line(b"net ipcfg static");
                self.addrs.our = s.static_ip;
                self.addrs.gw = s.static_gw;
                self.addrs.dns = s.static_dns;
                self.dhcp_phase = DhcpPhase::Idle;
            }
            IpConfig::Dhcp => {
                diag_log::line(b"net ipcfg dhcp");
                self.addrs = NetIpv4Addrs::ZERO;
                self.dhcp_phase = DhcpPhase::WaitOffer;
                self.dhcp_xid = self
                    .tick
                    .wrapping_mul(0x9E37_79B1)
                    ^ u32::from(self.tcp_seq);
                if self.dhcp_xid == 0 {
                    self.dhcp_xid = 0x0102_0304;
                }
                self.dhcp_server_id = [0; 4];
                self.dhcp_offer_yi = [0; 4];
                self.fetch_err_len = 0;
                self.dhcp_phase_start_tick = 0;
                self.dhcp_last_tx_tick = 0;
            }
        }
        self.reset_demo();
    }

    fn dhcp_active(&self) -> bool {
        matches!(
            self.dhcp_phase,
            DhcpPhase::WaitOffer | DhcpPhase::WaitAck
        )
    }

    fn dhcp_send_discover(
        &mut self,
        our_mac: &[u8; 6],
        scratch: &mut [u8],
        vio: &mut AnyNic,
    ) {
        let n = build_dhcp_packet(
            our_mac,
            NetIpv4Addrs::ZERO,
            self.dhcp_xid,
            our_mac,
            DHCP_DISCOVER,
            None,
            None,
            scratch,
        );
        if n > 0 {
            unsafe {
                let _ = vio.transmit(&scratch[..n]);
            }
        }
    }

    fn dhcp_send_request(
        &mut self,
        our_mac: &[u8; 6],
        scratch: &mut [u8],
        vio: &mut AnyNic,
    ) {
        let n = build_dhcp_packet(
            our_mac,
            NetIpv4Addrs::ZERO,
            self.dhcp_xid,
            our_mac,
            DHCP_REQUEST,
            Some(self.dhcp_server_id),
            Some(self.dhcp_offer_yi),
            scratch,
        );
        if n > 0 {
            unsafe {
                let _ = vio.transmit(&scratch[..n]);
            }
        }
    }

    fn handle_dhcp_reply(
        &mut self,
        bootp: &[u8],
        our_mac: &[u8; 6],
        scratch: &mut [u8],
        vio: &mut AnyNic,
    ) {
        if bootp.len() < 240 {
            return;
        }
        if bootp[0] != 2 {
            return;
        }
        let xid = u32::from_be_bytes([bootp[4], bootp[5], bootp[6], bootp[7]]);
        if xid != self.dhcp_xid {
            return;
        }
        if bootp[28..34] != our_mac[..6] {
            return;
        }
        let yi = [
            bootp[16], bootp[17], bootp[18], bootp[19],
        ];
        let mut msg = 0u8;
        let mut sid = [0u8; 4];
        let mut router = [0u8; 4];
        let mut dns = [0u8; 4];
        let mut have_sid = false;
        let mut have_router = false;
        let mut have_dns = false;
        if bootp.len() >= 244 && bootp[236..240] == DHCP_MAGIC {
            let mut i = 240usize;
            while i < bootp.len() {
                let tag = bootp[i];
                if tag == OPT_END {
                    break;
                }
                if tag == 0 {
                    i += 1;
                    continue;
                }
                if i + 1 >= bootp.len() {
                    break;
                }
                let ln = bootp[i + 1] as usize;
                let v0 = i + 2;
                if v0.saturating_add(ln) > bootp.len() {
                    break;
                }
                match tag {
                    OPT_MSG_TYPE if ln >= 1 => msg = bootp[v0],
                    OPT_SERVER_ID if ln >= 4 => {
                        sid.copy_from_slice(&bootp[v0..v0 + 4]);
                        have_sid = true;
                    }
                    OPT_ROUTER if ln >= 4 => {
                        router.copy_from_slice(&bootp[v0..v0 + 4]);
                        have_router = true;
                    }
                    OPT_DNS if ln >= 4 => {
                        dns.copy_from_slice(&bootp[v0..v0 + 4]);
                        have_dns = true;
                    }
                    _ => {}
                }
                i = v0 + ln;
            }
        }
        match self.dhcp_phase {
            DhcpPhase::WaitOffer if msg == DHCP_OFFER => {
                if !have_sid {
                    return;
                }
                self.dhcp_server_id = sid;
                self.dhcp_offer_yi = yi;
                self.dhcp_phase = DhcpPhase::WaitAck;
                self.dhcp_phase_start_tick = self.tick;
                self.dhcp_last_tx_tick = self.tick;
                self.dhcp_send_request(our_mac, scratch, vio);
            }
            DhcpPhase::WaitAck if msg == DHCP_ACK => {
                self.addrs.our = yi;
                let si = [
                    bootp[20], bootp[21], bootp[22], bootp[23],
                ];
                self.addrs.gw = if have_router {
                    router
                } else {
                    si
                };
                if self.addrs.gw == [0, 0, 0, 0] {
                    self.addrs.gw = self.addrs.our;
                }
                self.addrs.dns = if have_dns {
                    dns
                } else {
                    self.addrs.gw
                };
                self.dhcp_phase = DhcpPhase::Bound;
                self.gw_known = false;
                self.fetch_err_len = 0;
                diag_log::line(b"net dhcp bound");
                diag_log::ipv4(b"ip ", self.addrs.our);
                diag_log::ipv4(b"gw ", self.addrs.gw);
            }
            _ => {}
        }
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
        self.fetch_progress_tick = self.tick;
        self.http_bytes = 0;
        self.stream_len = 0;
        self.header_found = false;
        self.http_body_remaining = None;
        self.tcp_connect_start_tick = 0;
        self.page_len = 0;
        self.page.fill(0);
        self.stream.fill(0);
        self.fetch_err_len = 0;
        self.fetch_err.fill(0);
        self.needs_dns = false;
        self.dns_done = false;
        self.dns_xmit_phase = 0;
        self.page_truncated = false;
        self.body_layers = [BodyLayer::Identity; 4];
        self.body_layer_count = 0;
    }

    fn reset_tcp_for_new_fetch(&mut self) {
        self.syn_sent = false;
        self.get_sent = false;
        self.syn_retries = 0;
        self.tcp_connect_start_tick = self.tick;
        self.http_bytes = 0;
        self.stream_len = 0;
        self.header_found = false;
        self.http_body_remaining = None;
        self.page_len = 0;
        self.page.fill(0);
        self.stream.fill(0);
        self.fetch_done = false;
        self.fetch_err_len = 0;
        self.fetch_err.fill(0);
        self.page_truncated = false;
        self.body_layers = [BodyLayer::Identity; 4];
        self.body_layer_count = 0;
    }

    /// Parse `url` and start HTTP or HTTPS fetch. Errors copy a short message into `fetch_err`.
    pub fn start_fetch(&mut self, url: &[u8]) {
        self.clear_fetch_inner();
        let Some(p) = parse_fetch_url(url) else {
            self.set_err(b"BAD URL");
            return;
        };
        diag_log::fetch_host(&p.host_for_dns[..p.host_for_dns_len], p.https);
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
        diag_log::err_msg(msg);
        let n = msg.len().min(self.fetch_err.len());
        self.fetch_err[..n].copy_from_slice(&msg[..n]);
        self.fetch_err_len = n;
        self.fetch_armed = false;
        self.fetch_done = true;
        self.page_gen = self.page_gen.wrapping_add(1);
    }

    pub fn drive(
        &mut self,
        vio: &mut AnyNic,
        our_mac: &[u8; 6],
        scratch: &mut [u8],
        settings: &DeviceSettings,
    ) {
        self.sync_ip_from_settings(settings, settings.ip_settings_tag());
        self.tick = self.tick.wrapping_add(1);
        crate::eve_tls::wall_clock_note_net_tick(self.tick);

        loop {
            let n = match unsafe { vio.poll_rx_packet(&mut self.drive_rx_buf) } {
                None => break,
                Some(n) if n <= self.drive_rx_buf.len() => n,
                Some(_) => break,
            };
            // SAFETY: `handle_rx` uses `frame` only; it must not touch `drive_rx_buf` (TLS spin uses
            // `tls_spin_rx_buf`). Single-threaded kernel.
            let frame =
                unsafe { core::slice::from_raw_parts(self.drive_rx_buf.as_ptr(), n) };
            self.handle_rx(frame, our_mac, scratch, vio);
        }

        if settings.ip_config == IpConfig::Dhcp {
            match self.dhcp_phase {
                DhcpPhase::WaitOffer | DhcpPhase::WaitAck => {
                    if self.dhcp_phase_start_tick == 0 {
                        self.dhcp_phase_start_tick = self.tick;
                        self.dhcp_last_tx_tick = 0;
                    }
                    let due = self.dhcp_last_tx_tick == 0
                        || self.tick.wrapping_sub(self.dhcp_last_tx_tick) >= 200;
                    if due {
                        self.dhcp_last_tx_tick = self.tick;
                        match self.dhcp_phase {
                            DhcpPhase::WaitOffer => {
                                self.dhcp_send_discover(our_mac, scratch, vio);
                            }
                            DhcpPhase::WaitAck => {
                                self.dhcp_send_request(our_mac, scratch, vio);
                            }
                            _ => {}
                        }
                    }
                    if self
                        .tick
                        .wrapping_sub(self.dhcp_phase_start_tick)
                        > 12_000
                    {
                        // No DHCP reply (misconfigured netdev, slow firmware, etc.): use SLIRP
                        // triple so VirtIO user-NAT / QEMU 10.0.2.0/24 still works without SYS edits.
                        diag_log::line(b"net dhcp timeout -> slirp");
                        self.addrs = NetIpv4Addrs::SLIRP;
                        self.dhcp_phase = DhcpPhase::Idle;
                        self.gw_known = false;
                        self.dhcp_phase_start_tick = 0;
                        self.dhcp_last_tx_tick = 0;
                    }
                }
                _ => {}
            }
            if matches!(self.dhcp_phase, DhcpPhase::WaitOffer | DhcpPhase::WaitAck) {
                return;
            }
        }

        if self.addrs.is_our_zero() {
            return;
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
                let len = build_arp_request(our_mac, self.addrs, scratch);
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
                    self.addrs,
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
            // ~15s at ~120 frames/s — avoids hanging forever if virtio TX fails after first SYN.
            const TCP_CONNECT_TICKS: u32 = 16_000;
            if !self.get_sent
                && self.tick.wrapping_sub(self.tcp_connect_start_tick) > TCP_CONNECT_TICKS
            {
                self.set_err(b"TCP NO CONNECT");
                return;
            }
            let try_syn = !self.syn_sent
                || (self.syn_retries < 12 && self.tick % 96 == 0 && !self.get_sent);
            if try_syn {
                let len = build_tcp_syn(
                    our_mac,
                    &self.gw_mac,
                    self.addrs,
                    self.tcp_seq,
                    self.remote_ip,
                    self.remote_port,
                    scratch,
                );
                if len > 0 {
                    let _ = unsafe { vio.transmit(&scratch[..len]) };
                    self.syn_sent = true;
                    self.syn_retries = self.syn_retries.saturating_add(1);
                }
            }

            // Connected but no HTTP payload progress for too long: surface a user-visible error.
            if self.get_sent && self.tick.wrapping_sub(self.fetch_progress_tick) > 12_000 {
                self.set_err(b"HTTP TIMEOUT");
                return;
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
        vio: &mut AnyNic,
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
        let sip = [frame[26], frame[27], frame[28], frame[29]];
        let proto = frame[23];

        if proto == 17 {
            let u0 = 14 + ihl;
            if frame.len() >= u0 + 8 {
                let sport = u16::from_be_bytes([frame[u0], frame[u0 + 1]]);
                let dport = u16::from_be_bytes([frame[u0 + 2], frame[u0 + 3]]);
                if sport == DHCP_SERVER_PORT
                    && dport == DHCP_CLIENT_PORT
                    && self.dhcp_active()
                {
                    let udp_len = u16::from_be_bytes([frame[u0 + 4], frame[u0 + 5]]) as usize;
                    if udp_len >= 8 && u0 + udp_len <= frame.len() {
                        let payload = &frame[u0 + 8..u0 + udp_len];
                        self.handle_dhcp_reply(payload, our_mac, scratch, vio);
                    }
                    return;
                }
            }
        }

        let dip_ok = dip == self.addrs.our || dip == [255, 255, 255, 255];
        if !dip_ok {
            return;
        }

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
        _vio: &mut AnyNic,
    ) {
        if sip != self.addrs.dns || !self.fetch_armed || !self.needs_dns || self.dns_done {
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
        vio: &mut AnyNic,
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
                    self.addrs,
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
            let mut pay = [0u8; 512];
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
                self.addrs,
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
            self.fetch_progress_tick = self.tick;
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
                self.addrs,
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
                self.addrs,
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
        self.tls_poll_nic = core::ptr::null_mut();
        self.tls_poll_scratch = core::ptr::null_mut();
        self.tls_poll_scratch_len = 0;
    }

    fn set_tls_poll(&mut self, vio: *mut AnyNic, mac: &[u8; 6], scratch: &mut [u8]) {
        self.tls_poll_nic = vio;
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

    fn tls_tx_flush_all(&mut self, vio: &mut AnyNic, our_mac: &[u8; 6], scratch: &mut [u8]) {
        const MSS: usize = 1400;
        while self.tls_tx_len > 0 {
            let n = self.tls_tx_len.min(MSS);
            let len = build_tcp_ack_psh(
                our_mac,
                &self.gw_mac,
                self.addrs,
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
        if self.tls_poll_nic.is_null() {
            return;
        }
        let poll_mac = self.tls_poll_mac;
        unsafe {
            let vio = &mut *self.tls_poll_nic;
            let scratch = core::slice::from_raw_parts_mut(
                self.tls_poll_scratch,
                self.tls_poll_scratch_len,
            );
            self.tls_tx_flush_all(vio, &poll_mac, scratch);
            loop {
                let n = match vio.poll_rx_packet(&mut self.tls_spin_rx_buf) {
                    None => break,
                    Some(n) if n <= self.tls_spin_rx_buf.len() => n,
                    Some(_) => break,
                };
                let frame =
                    core::slice::from_raw_parts(self.tls_spin_rx_buf.as_ptr(), n);
                self.handle_rx(frame, &poll_mac, scratch, vio);
            }
        }
    }

    pub(crate) fn tls_eof_from_tcp(&self) -> bool {
        self.tls_tcp_eof
    }

    pub(crate) fn tls_note_flush_pending(&mut self) {
        self.tls_tx_flush_pending = true;
    }

    fn run_https_handshake_and_get(&mut self, vio: &mut AnyNic, our_mac: &[u8; 6], scratch: &mut [u8]) {
        self.set_tls_poll(vio as *mut AnyNic, our_mac, scratch);
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
        // Try common roots: LE (ISRG X1) first, then DigiCert G2.
        let config_isrg = TlsConfig::new()
            .with_server_name(host)
            .with_ca(Certificate::X509(ISRG_ROOT_X1_DER));
        let config_digicert = TlsConfig::new()
            .with_server_name(host)
            .with_ca(Certificate::X509(DIGICERT_GLOBAL_ROOT_G2_DER));
        let bridge = TlsNetBridge {
            net: self as *mut NetStack,
        };
        let tls_r = core::ptr::addr_of_mut!(self.tls_rbuf);
        let tls_w = core::ptr::addr_of_mut!(self.tls_wbuf);
        let mut tls = unsafe { TlsConnection::new(bridge, &mut *tls_r, &mut *tls_w) };
        let mac_le = u64::from_le_bytes([
            our_mac[0], our_mac[1], our_mac[2], our_mac[3], our_mac[4], our_mac[5], 0, 0,
        ]);
        let seed =
            u64::from(self.tcp_seq) ^ u64::from(self.tick).rotate_left(17) ^ mac_le;
        let mut ok = false;
        if tls
            .open(TlsContext::new(
                &config_isrg,
                EveVerifiedTlsProvider::new(seed ^ 0x1A2B_3C4D),
            ))
            .is_ok()
        {
            ok = true;
        } else {
            let bridge2 = TlsNetBridge {
                net: self as *mut NetStack,
            };
            let mut tls2 = unsafe { TlsConnection::new(bridge2, &mut *tls_r, &mut *tls_w) };
            if tls2
                .open(TlsContext::new(
                    &config_digicert,
                    EveVerifiedTlsProvider::new(seed ^ 0xD1C1_CE47),
                ))
                .is_ok()
            {
                tls = tls2;
                ok = true;
            }
        }
        if !ok {
            self.clear_tls_poll();
            self.set_err(b"TLS VERIFY FAIL");
            return;
        }
        self.tls.write(tls);
        self.tls_live = true;
        self.tls_handshake_done = true;

        let mut pay = [0u8; 512];
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

    fn tls_pump_application(&mut self, vio: &mut AnyNic, our_mac: &[u8; 6], scratch: &mut [u8]) {
        if !self.tls_live {
            return;
        }
        self.set_tls_poll(vio as *mut AnyNic, our_mac, scratch);
        self.tls_spin_poll();
        loop {
            let n = unsafe {
                match self.tls.assume_init_mut().read(&mut self.tls_read_tmp) {
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
            let chunk = unsafe { core::slice::from_raw_parts(self.tls_read_tmp.as_ptr(), n) };
            self.ingest_tcp_payload(chunk);
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
        if self.body_layer_count > 0 {
            match self.decompress_body_layers() {
                Ok(()) => {}
                Err(_) if body_starts_like_html(&self.page[..self.page_len]) => {
                    // `Content-Encoding` did not match the wire bytes (cleartext HTML with gzip label).
                    self.body_layer_count = 0;
                    self.body_layers = [BodyLayer::Identity; 4];
                }
                Err(msg) => {
                    self.set_err(msg);
                    return;
                }
            }
        } else if self.page_len >= 2 && self.page[0] == 0x1f && self.page[1] == 0x8b {
            // gzip member without `Content-Encoding` (some stacks omit it).
            if let Err(msg) = self.decompress_gzip_once() {
                self.set_err(msg);
                return;
            }
        }
        self.fetch_done = true;
        self.fetch_armed = false;
        self.page_gen = self.page_gen.wrapping_add(1);
    }

    /// Apply `Content-Encoding` layers (outermost / last header token first).
    fn decompress_body_layers(&mut self) -> Result<(), &'static [u8]> {
        let n = self.body_layer_count as usize;
        for i in (0..n).rev() {
            match self.body_layers[i] {
                BodyLayer::Identity => {}
                BodyLayer::Gzip => self.decompress_gzip_once()?,
                BodyLayer::DeflateZlib => self.decompress_zlib_once()?,
                BodyLayer::Brotli => self.decompress_brotli_once()?,
            }
        }
        Ok(())
    }

    /// Gunzip current [`Self::page`][..`page_len`] into scratch, then copy plain bytes back.
    fn decompress_gzip_once(&mut self) -> Result<(), &'static [u8]> {
        let comp_len = self.page_len;
        if comp_len < 18 {
            return Err(b"GZIP SHORT");
        }
        let comp = &self.page[..comp_len];
        let Some(deflate) = gzip_deflate_payload(comp) else {
            return Err(b"GZIP HDR");
        };
        let out = &mut self.gunzip_buf[..];
        match decompress_slice_iter_to_slice(out, core::iter::once(deflate), false, true) {
            Ok(n) => {
                if n > self.page.len() {
                    return Err(b"GZIP INT");
                }
                self.page[..n].copy_from_slice(&out[..n]);
                self.page_len = n;
                Ok(())
            }
            Err(TINFLStatus::HasMoreOutput) => Err(b"GZIP BIG"),
            Err(_) => Err(b"GZIP BAD"),
        }
    }

    /// zlib (RFC 1950) wrapper around DEFLATE for HTTP `deflate`.
    fn decompress_zlib_once(&mut self) -> Result<(), &'static [u8]> {
        let inp = &self.page[..self.page_len];
        let out = &mut self.gunzip_buf[..];
        match decompress_slice_iter_to_slice(out, core::iter::once(inp), true, true) {
            Ok(n) => {
                if n > self.page.len() {
                    return Err(b"ZLIB INT");
                }
                self.page[..n].copy_from_slice(&out[..n]);
                self.page_len = n;
                Ok(())
            }
            Err(TINFLStatus::HasMoreOutput) => Err(b"ZLIB BIG"),
            Err(_) => Err(b"ZLIB BAD"),
        }
    }

    fn decompress_brotli_once(&mut self) -> Result<(), &'static [u8]> {
        let input = &self.page[..self.page_len];
        let out = &mut self.gunzip_buf[..];
        let info = brotli_decode_prealloc(
            input,
            out,
            &mut self.brotli_scratch_u8[..],
            &mut self.brotli_scratch_u32[..],
            &mut self.brotli_scratch_huffman[..],
        );
        if !matches!(info.result, BrotliResult::ResultSuccess) {
            return Err(b"BR BAD");
        }
        let n = info.decoded_size;
        if n > self.page.len() {
            return Err(b"BR INT");
        }
        self.page[..n].copy_from_slice(&out[..n]);
        self.page_len = n;
        Ok(())
    }

    fn ingest_tcp_payload(&mut self, data: &[u8]) {
        if !self.header_found {
            let room = self.stream.len().saturating_sub(self.stream_len);
            let n = data.len().min(room);
            self.stream[self.stream_len..self.stream_len + n].copy_from_slice(&data[..n]);
            self.stream_len += n;
            if self.stream_len >= self.stream.len()
                && find_http_header_end(&self.stream[..self.stream_len]).is_none()
            {
                self.set_err(b"HTTP HDR TOO BIG");
                return;
            }
            if let Some((pos, sep_len)) = find_http_header_end(&self.stream[..self.stream_len]) {
                self.header_found = true;
                let hdr_end = pos + sep_len;
                let headers = &self.stream[..hdr_end];
                let body_off = hdr_end;
                let end = self.stream_len;
                let body_in_header_buf = end.saturating_sub(body_off);
                if response_headers_chunked(headers) {
                    self.set_err(b"HTTP CHUNKED");
                    self.stream_len = 0;
                    self.header_found = true;
                    self.finish_fetch();
                    return;
                }
                match parse_content_encoding_layers(headers) {
                    Ok((layers, count)) => {
                        self.body_layers = layers;
                        self.body_layer_count = count;
                    }
                    Err(()) => {
                        self.set_err(b"HTTP ENC");
                        return;
                    }
                }
                if let Some(cl) = parse_content_length(headers) {
                    self.http_body_remaining = Some(cl.saturating_sub(body_in_header_buf.min(cl)));
                } else {
                    self.http_body_remaining = None;
                }
                for i in body_off..end {
                    self.page_push_byte(self.stream[i]);
                }
                self.stream_len = 0;
                self.page_gen = self.page_gen.wrapping_add(1);
                if self.http_body_remaining == Some(0) {
                    self.finish_fetch();
                }
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
        if let Some(rem) = &mut self.http_body_remaining {
            let take = (*rem).min(n);
            *rem -= take;
            if *rem == 0 {
                self.finish_fetch();
            }
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
        if tpa != self.addrs.our {
            return;
        }
        let spa = [frame[a + 14], frame[a + 15], frame[a + 16], frame[a + 17]];
        if spa != self.addrs.gw {
            return;
        }
        self.gw_mac.copy_from_slice(&frame[a + 8..a + 14]);
        self.gw_known = true;
    }
}

fn find_http_header_end(buf: &[u8]) -> Option<(usize, usize)> {
    if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
        return Some((pos, 4));
    }
    if let Some(pos) = buf.windows(2).position(|w| w == b"\n\n") {
        return Some((pos, 2));
    }
    None
}

fn line_starts_with_ci(line: &[u8], prefix: &[u8]) -> bool {
    line.len() >= prefix.len()
        && line[..prefix.len()]
            .iter()
            .zip(prefix.iter())
            .all(|(a, b)| a.to_ascii_lowercase() == *b)
}

fn trim_http_line_prefix<'a>(line: &'a [u8], prefix: &[u8]) -> Option<&'a [u8]> {
    if line_starts_with_ci(line, prefix) {
        let mut rest = &line[prefix.len()..];
        while rest.first() == Some(&b' ') || rest.first() == Some(&b'\t') {
            rest = &rest[1..];
        }
        Some(rest)
    } else {
        None
    }
}

/// `true` if `Transfer-Encoding: …` contains `chunked` (HTTP/1.1). Chunk decoding is not
/// implemented; the fetcher stops with [`NetStack::fetch_err`] `HTTP CHUNKED` instead of showing
/// garbled body data.
fn response_headers_chunked(headers: &[u8]) -> bool {
    let mut i = 0usize;
    while i < headers.len() {
        let rest = &headers[i..];
        let line_end = rest
            .iter()
            .position(|&b| b == b'\n')
            .map(|p| i + p)
            .unwrap_or(headers.len());
        let mut line = &headers[i..line_end];
        if line.last() == Some(&b'\r') {
            line = &line[..line.len().saturating_sub(1)];
        }
        let Some(te) = trim_http_line_prefix(line, b"transfer-encoding:") else {
            i = line_end.saturating_add(1);
            continue;
        };
        let v = te.split(|&b| b == b',');
        for part in v {
            let mut p = part;
            while p.first() == Some(&b' ') || p.first() == Some(&b'\t') {
                p = &p[1..];
            }
            while p.last() == Some(&b' ') || p.last() == Some(&b'\t') {
                p = &p[..p.len().saturating_sub(1)];
            }
            if line_starts_with_ci(p, b"chunked") {
                return true;
            }
        }
        i = line_end.saturating_add(1);
    }
    false
}

fn parse_usize_decimal(mut s: &[u8]) -> Option<usize> {
    while s.first() == Some(&b' ') || s.first() == Some(&b'\t') {
        s = &s[1..];
    }
    if s.is_empty() {
        return None;
    }
    let mut v: usize = 0;
    for &c in s {
        if c == b' ' || c == b'\t' {
            break;
        }
        if !c.is_ascii_digit() {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((c - b'0') as usize)?;
    }
    Some(v)
}

/// First `Content-Length:` value in response headers (bytes). Ignored when chunked.
fn parse_content_length(headers: &[u8]) -> Option<usize> {
    let mut i = 0usize;
    while i < headers.len() {
        let rest = &headers[i..];
        let line_end = rest
            .iter()
            .position(|&b| b == b'\n')
            .map(|p| i + p)
            .unwrap_or(headers.len());
        let mut line = &headers[i..line_end];
        if line.last() == Some(&b'\r') {
            line = &line[..line.len().saturating_sub(1)];
        }
        if let Some(v) = trim_http_line_prefix(line, b"content-length:") {
            if let Some(n) = parse_usize_decimal(v) {
                return Some(n);
            }
        }
        i = line_end.saturating_add(1);
    }
    None
}

fn token_bytes_ci_eq(token: &[u8], expect: &[u8]) -> bool {
    token.len() == expect.len()
        && token
            .iter()
            .zip(expect.iter())
            .all(|(a, b)| a.to_ascii_lowercase() == *b)
}

/// Parse `Content-Encoding` into up to four layers (application order). Returns `Err` on unknown
/// codings or more than four layers.
fn parse_content_encoding_layers(headers: &[u8]) -> Result<([BodyLayer; 4], u8), ()> {
    let mut layers = [BodyLayer::Identity; 4];
    let mut count: usize = 0;
    let mut i = 0usize;
    while i < headers.len() {
        let rest = &headers[i..];
        let line_end = rest
            .iter()
            .position(|&b| b == b'\n')
            .map(|p| i + p)
            .unwrap_or(headers.len());
        let mut line = &headers[i..line_end];
        if line.last() == Some(&b'\r') {
            line = &line[..line.len().saturating_sub(1)];
        }
        if let Some(v) = trim_http_line_prefix(line, b"content-encoding:") {
            for raw_part in v.split(|&b| b == b',') {
                let mut p = raw_part;
                while p.first() == Some(&b' ') || p.first() == Some(&b'\t') {
                    p = &p[1..];
                }
                while p.last() == Some(&b' ') || p.last() == Some(&b'\t') {
                    p = &p[..p.len().saturating_sub(1)];
                }
                if let Some(semi) = p.iter().position(|&x| x == b';') {
                    p = &p[..semi];
                }
                while p.first() == Some(&b' ') || p.first() == Some(&b'\t') {
                    p = &p[1..];
                }
                while p.last() == Some(&b' ') || p.last() == Some(&b'\t') {
                    p = &p[..p.len().saturating_sub(1)];
                }
                if p.is_empty() {
                    continue;
                }
                let layer = if token_bytes_ci_eq(p, b"gzip") || token_bytes_ci_eq(p, b"x-gzip") {
                    BodyLayer::Gzip
                } else if token_bytes_ci_eq(p, b"deflate") {
                    BodyLayer::DeflateZlib
                } else if token_bytes_ci_eq(p, b"br") {
                    BodyLayer::Brotli
                } else if token_bytes_ci_eq(p, b"identity") || token_bytes_ci_eq(p, b"compress") {
                    continue;
                } else {
                    return Err(());
                };
                if count >= layers.len() {
                    return Err(());
                }
                layers[count] = layer;
                count += 1;
            }
        }
        i = line_end.saturating_add(1);
    }
    Ok((layers, count as u8))
}

fn body_starts_like_html(buf: &[u8]) -> bool {
    let mut s = buf;
    while let Some(&b) = s.first() {
        if b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' {
            s = &s[1..];
        } else {
            break;
        }
    }
    s.first() == Some(&b'<')
}

/// RFC 1952 gzip member: raw DEFLATE bitstream (no zlib wrapper), excluding 8-byte trailer.
fn gzip_deflate_payload(gzip: &[u8]) -> Option<&[u8]> {
    const MIN: usize = 10 + 8;
    if gzip.len() < MIN {
        return None;
    }
    if gzip[0] != 0x1f || gzip[1] != 0x8b || gzip[2] != 8 {
        return None;
    }
    let flg = gzip[3];
    if flg & 0xE0 != 0 {
        return None;
    }
    let mut i = 10usize;
    if flg & 4 != 0 {
        if i + 2 > gzip.len() {
            return None;
        }
        let xlen = u16::from_le_bytes([gzip[i], gzip[i + 1]]) as usize;
        i = i.checked_add(2)?.checked_add(xlen)?;
        if i > gzip.len() {
            return None;
        }
    }
    if flg & 8 != 0 {
        while i < gzip.len() && gzip[i] != 0 {
            i += 1;
        }
        if i >= gzip.len() {
            return None;
        }
        i += 1;
    }
    if flg & 16 != 0 {
        while i < gzip.len() && gzip[i] != 0 {
            i += 1;
        }
        if i >= gzip.len() {
            return None;
        }
        i += 1;
    }
    if flg & 2 != 0 {
        if i + 2 > gzip.len() {
            return None;
        }
        i += 2;
    }
    let body_end = gzip.len().checked_sub(8)?;
    if i > body_end {
        return None;
    }
    Some(&gzip[i..body_end])
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
    // Prefer **gzip** only: some CDNs pick **br** or **deflate** from a long list; our stack is
    // much better tested on gzip, and mis-picked encodings leave binary in `page` → blank browser.
    const P3: &[u8] = b"\r\nUser-Agent: EveOS/0.1\r\nAccept-Encoding: gzip, identity\r\nConnection: close\r\n\r\n";
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

fn build_arp_request(our_mac: &[u8; 6], addrs: NetIpv4Addrs, out: &mut [u8]) -> usize {
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
    out[o + 28..o + 32].copy_from_slice(&addrs.our);
    out[o + 32..o + 38].fill(0);
    out[o + 38..o + 42].copy_from_slice(&addrs.gw);
    total
}

fn build_tcp_syn(
    our_mac: &[u8; 6],
    gw_mac: &[u8; 6],
    addrs: NetIpv4Addrs,
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
    out[ip + 12..ip + 16].copy_from_slice(&addrs.our);
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
    let ph = pseudo_sum(addrs.our, remote_ip, 6, tcp_len as u16);
    let tc = sum16(&out[t..t + tcp_len], ph as u32);
    out[t + 16..t + 18].copy_from_slice(&tc.to_be_bytes());
    total
}

fn build_tcp_ack_psh(
    our_mac: &[u8; 6],
    gw_mac: &[u8; 6],
    addrs: NetIpv4Addrs,
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
    out[ip + 12..ip + 16].copy_from_slice(&addrs.our);
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
    let ph = pseudo_sum(addrs.our, remote_ip, 6, tcp_len as u16);
    let tc = sum16(&out[t..t + tcp_len], ph as u32);
    out[t + 16..t + 18].copy_from_slice(&tc.to_be_bytes());
    total
}

fn build_tcp_ack_only(
    our_mac: &[u8; 6],
    gw_mac: &[u8; 6],
    addrs: NetIpv4Addrs,
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
    out[ip + 12..ip + 16].copy_from_slice(&addrs.our);
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
    let ph = pseudo_sum(addrs.our, remote_ip, 6, tcp_len as u16);
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

fn dhcp_push_opt(buf: &mut [u8], pos: &mut usize, tag: u8, data: &[u8]) -> bool {
    if *pos + 2 + data.len() > buf.len() {
        return false;
    }
    buf[*pos] = tag;
    buf[*pos + 1] = data.len() as u8;
    buf[*pos + 2..*pos + 2 + data.len()].copy_from_slice(data);
    *pos += 2 + data.len();
    true
}

/// UDP/IPv4 broadcast DHCP (DISCOVER / REQUEST). `ip_src` uses `.our` as IPv4 source (often `0.0.0.0`).
fn build_dhcp_packet(
    our_mac: &[u8; 6],
    ip_src: NetIpv4Addrs,
    xid: u32,
    client_mac: &[u8; 6],
    msg_type: u8,
    server_id: Option<[u8; 4]>,
    requested_ip: Option<[u8; 4]>,
    out: &mut [u8],
) -> usize {
    let mut bp = [0u8; 576];
    bp[0] = 1;
    bp[1] = 1;
    bp[2] = 6;
    bp[4..8].copy_from_slice(&xid.to_be_bytes());
    bp[10..12].copy_from_slice(&0x8000u16.to_be_bytes());
    bp[28..34].copy_from_slice(&client_mac[..6]);
    bp[236..240].copy_from_slice(&DHCP_MAGIC);
    let mut o = 240usize;
    if !dhcp_push_opt(&mut bp, &mut o, OPT_MSG_TYPE, &[msg_type]) {
        return 0;
    }
    if let Some(ip) = requested_ip {
        if !dhcp_push_opt(&mut bp, &mut o, OPT_REQUESTED_IP, &ip) {
            return 0;
        }
    }
    if let Some(sid) = server_id {
        if !dhcp_push_opt(&mut bp, &mut o, OPT_SERVER_ID, &sid) {
            return 0;
        }
    }
    if o >= bp.len() {
        return 0;
    }
    bp[o] = OPT_END;
    o += 1;
    let plen = o;
    let udp_len = 8 + plen;
    let ip_len = 20 + udp_len;
    let eth_len = 14 + ip_len;
    let total = VIRTIO_NET_HDR + eth_len;
    if out.len() < total {
        return 0;
    }
    let ip_dst = [255u8, 255, 255, 255];
    let bcast = [0xffu8; 6];
    out[..VIRTIO_NET_HDR].fill(0);
    let e = VIRTIO_NET_HDR;
    out[e..e + 6].copy_from_slice(&bcast);
    out[e + 6..e + 12].copy_from_slice(our_mac);
    out[e + 12..e + 14].copy_from_slice(&ETH_P_IP.to_be_bytes());
    let ip = e + 14;
    out[ip] = 0x45;
    out[ip + 1] = 0;
    out[ip + 2..ip + 4].copy_from_slice(&(ip_len as u16).to_be_bytes());
    out[ip + 4..ip + 6].copy_from_slice(&0u16.to_be_bytes());
    out[ip + 6..ip + 8].copy_from_slice(&0u16.to_be_bytes());
    out[ip + 8] = 64;
    out[ip + 9] = 17;
    out[ip + 10..ip + 12].copy_from_slice(&0u16.to_be_bytes());
    out[ip + 12..ip + 16].copy_from_slice(&ip_src.our);
    out[ip + 16..ip + 20].copy_from_slice(&ip_dst);
    let csum = sum16(&out[ip..ip + 20], 0);
    out[ip + 10..ip + 12].copy_from_slice(&csum.to_be_bytes());
    let u = ip + 20;
    out[u..u + 2].copy_from_slice(&DHCP_CLIENT_PORT.to_be_bytes());
    out[u + 2..u + 4].copy_from_slice(&DHCP_SERVER_PORT.to_be_bytes());
    out[u + 4..u + 6].copy_from_slice(&(udp_len as u16).to_be_bytes());
    out[u + 6..u + 8].copy_from_slice(&0u16.to_be_bytes());
    out[u + 8..u + 8 + plen].copy_from_slice(&bp[..plen]);
    let ph = pseudo_sum(ip_src.our, ip_dst, 17, udp_len as u16);
    let ucsum = sum16(&out[u..u + udp_len], ph as u32);
    out[u + 6..u + 8].copy_from_slice(&ucsum.to_be_bytes());
    total
}

fn build_dns_udp_packet(
    our_mac: &[u8; 6],
    gw_mac: &[u8; 6],
    addrs: NetIpv4Addrs,
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
    out[ip + 12..ip + 16].copy_from_slice(&addrs.our);
    out[ip + 16..ip + 20].copy_from_slice(&addrs.dns);
    let csum = sum16(&out[ip..ip + 20], 0);
    out[ip + 10..ip + 12].copy_from_slice(&csum.to_be_bytes());

    let u = ip + 20;
    out[u..u + 2].copy_from_slice(&sport.to_be_bytes());
    out[u + 2..u + 4].copy_from_slice(&dport.to_be_bytes());
    out[u + 4..u + 6].copy_from_slice(&(udp_len as u16).to_be_bytes());
    out[u + 6..u + 8].copy_from_slice(&0u16.to_be_bytes());
    out[u + 8..u + 8 + dp].copy_from_slice(&dns[..dp]);

    let ph = pseudo_sum(addrs.our, addrs.dns, 17, udp_len as u16);
    let ucsum = sum16(&out[u..u + udp_len], ph as u32);
    out[u + 6..u + 8].copy_from_slice(&ucsum.to_be_bytes());

    total
}
