// SPDX-License-Identifier: MIT OR Apache-2.0

//! ARP + minimal TCP/HTTP client for QEMU user networking (`10.0.2.0/24`, gateway `.2`).
//! Wi‑Fi vs Ethernet: both use this path when VirtIO is up; real 802.11 is not implemented.

use crate::virtio_net::VirtioNet;

pub const VIRTIO_NET_HDR: usize = 12;

const OUR_IP: [u8; 4] = [10, 0, 2, 15];
const GW_IP: [u8; 4] = [10, 0, 2, 2];
/// example.com — HTTP/80 for a simple “internet works” check (no TLS in Eve).
const REMOTE_IP: [u8; 4] = [93, 184, 216, 34];
const REMOTE_PORT: u16 = 80;
const LOCAL_PORT: u16 = 49152;

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
    pub http_bytes: u32,
    syn_retries: u8,
    pub phase: NetPhase,
}

impl NetStack {
    pub const fn new() -> Self {
        Self {
            gw_mac: [0; 6],
            gw_known: false,
            tick: 0,
            ip_id: 0x4000,
            tcp_seq: 0,
            tcp_ack: 0,
            syn_sent: false,
            get_sent: false,
            http_bytes: 0,
            syn_retries: 0,
            phase: NetPhase::Off,
        }
    }

    pub fn seed_from_mac(&mut self, mac: &[u8; 6]) {
        let mut s = 0x1234_5678u32;
        for b in mac.iter() {
            s = s.wrapping_mul(0x0100_0193).wrapping_add(u32::from(*b));
        }
        self.tcp_seq = s;
    }

    /// Drop gateway / TCP state so ARP and HTTP run again (browser “R”).
    pub fn reset_demo(&mut self) {
        self.gw_mac = [0; 6];
        self.gw_known = false;
        self.tick = 0;
        self.syn_sent = false;
        self.get_sent = false;
        self.http_bytes = 0;
        self.syn_retries = 0;
        self.phase = NetPhase::Off;
    }

    pub fn drive(&mut self, vio: &mut VirtioNet, our_mac: &[u8; 6], scratch: &mut [u8]) {
        self.tick = self.tick.wrapping_add(1);
        if !self.gw_known {
            self.phase = NetPhase::Arp;
        } else if self.get_sent && self.http_bytes > 0 {
            self.phase = NetPhase::Done;
        } else if !self.get_sent {
            self.phase = NetPhase::Tcp;
        } else {
            self.phase = NetPhase::Http;
        }

        let mut rxb = [0u8; 2048];
        while let Some(n) = unsafe { vio.poll_rx_packet(&mut rxb) } {
            self.handle_rx(&rxb[..n], our_mac, scratch, vio);
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

        if !self.syn_sent || (self.syn_retries < 12 && self.tick % 96 == 0 && !self.get_sent) {
            let len = build_tcp_syn(our_mac, &self.gw_mac, self.tcp_seq, scratch);
            if len > 0 && unsafe { vio.transmit(&scratch[..len]) } {
                self.syn_sent = true;
                self.syn_retries = self.syn_retries.saturating_add(1);
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
        if frame[14] >> 4 != 4 || frame[23] != 6 {
            return;
        }
        let ihl = (frame[14] & 0x0f) as usize * 4;
        if frame.len() < 14 + ihl + 20 {
            return;
        }
        let dip = [frame[30], frame[31], frame[32], frame[33]];
        if dip != OUR_IP {
            return;
        }
        let sip = [frame[26], frame[27], frame[28], frame[29]];
        let tcp_off = 14 + ihl;
        let sport = u16::from_be_bytes([frame[tcp_off], frame[tcp_off + 1]]);
        let dport = u16::from_be_bytes([frame[tcp_off + 2], frame[tcp_off + 3]]);
        if dport != LOCAL_PORT {
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

        if sip != REMOTE_IP || sport != REMOTE_PORT {
            return;
        }

        if (flg & 0x12) == 0x12 && !self.get_sent {
            self.tcp_ack = seq.wrapping_add(1);
            self.tcp_seq = self.tcp_seq.wrapping_add(1);
            let pay = b"GET / HTTP/1.0\r\nHost: example.com\r\n\r\n";
            let len = build_tcp_ack_psh(
                our_mac,
                &self.gw_mac,
                sip,
                LOCAL_PORT,
                REMOTE_PORT,
                self.tcp_seq,
                self.tcp_ack,
                pay,
                self.ip_id.wrapping_add(1),
                scratch,
            );
            self.ip_id = self.ip_id.wrapping_add(1);
            if len > 0 && unsafe { vio.transmit(&scratch[..len]) } {
                self.get_sent = true;
                self.tcp_seq = self.tcp_seq.wrapping_add(pay.len() as u32);
            }
            return;
        }

        if (flg & 0x10) != 0 && frame.len() > tcp_off + hlen {
            let dlen = frame.len() - (tcp_off + hlen);
            self.http_bytes = self.http_bytes.wrapping_add(dlen as u32);
            self.tcp_ack = seq.wrapping_add(dlen as u32);
            let len = build_tcp_ack_only(
                our_mac,
                &self.gw_mac,
                sip,
                LOCAL_PORT,
                REMOTE_PORT,
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

fn build_tcp_syn(our_mac: &[u8; 6], gw_mac: &[u8; 6], seq: u32, out: &mut [u8]) -> usize {
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
    out[ip + 16..ip + 20].copy_from_slice(&REMOTE_IP);
    let csum = sum16(&out[ip..ip + 20], 0);
    out[ip + 10..ip + 12].copy_from_slice(&csum.to_be_bytes());

    let t = ip + 20;
    out[t..t + 2].copy_from_slice(&LOCAL_PORT.to_be_bytes());
    out[t + 2..t + 4].copy_from_slice(&REMOTE_PORT.to_be_bytes());
    out[t + 4..t + 8].copy_from_slice(&seq.to_be_bytes());
    out[t + 8..t + 12].copy_from_slice(&0u32.to_be_bytes());
    out[t + 12] = 0x50;
    out[t + 13] = 0x02;
    out[t + 14..t + 16].copy_from_slice(&0x2000u16.to_be_bytes());
    out[t + 16..t + 18].copy_from_slice(&0u16.to_be_bytes());
    out[t + 18..t + 20].copy_from_slice(&0u16.to_be_bytes());
    let ph = pseudo_sum(OUR_IP, REMOTE_IP, 6, tcp_len as u16);
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

fn pseudo_sum(src: [u8; 4], dst: [u8; 4], proto: u8, tcp_len: u16) -> u32 {
    let mut s = 0u32;
    s += u16::from_be_bytes([src[0], src[1]]) as u32;
    s += u16::from_be_bytes([src[2], src[3]]) as u32;
    s += u16::from_be_bytes([dst[0], dst[1]]) as u32;
    s += u16::from_be_bytes([dst[2], dst[3]]) as u32;
    s += u32::from(proto);
    s += u32::from(tcp_len);
    s
}
