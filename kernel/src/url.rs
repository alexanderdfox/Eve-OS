// SPDX-License-Identifier: MIT OR Apache-2.0

//! Parse `http://host[:port]/path` for in-guest fetch (no TLS).

#[derive(Clone, Copy)]
pub struct ParsedHttpUrl {
    pub port: u16,
    /// Resolved IPv4 when `needs_dns` is false; zeros when DNS still required.
    pub ip: [u8; 4],
    pub needs_dns: bool,
    pub host_for_dns: [u8; 96],
    pub host_for_dns_len: usize,
    /// Exact bytes for `Host:` header (host[:port] or host).
    pub host_header: [u8; 96],
    pub host_header_len: usize,
    pub path: [u8; 160],
    pub path_len: usize,
}

fn eq_ci(a: u8, b: u8) -> bool {
    a.to_ascii_lowercase() == b.to_ascii_lowercase()
}

fn starts_with_http(url: &[u8]) -> Option<&[u8]> {
    const P: &[u8] = b"http://";
    if url.len() < P.len() {
        return None;
    }
    for i in 0..P.len() {
        if !eq_ci(url[i], P[i]) {
            return None;
        }
    }
    Some(&url[P.len()..])
}

fn parse_u8(s: &[u8]) -> Option<u8> {
    if s.is_empty() || s.len() > 3 {
        return None;
    }
    let mut v = 0u16;
    for &c in s {
        if !c.is_ascii_digit() {
            return None;
        }
        v = v * 10 + u16::from(c - b'0');
        if v > 255 {
            return None;
        }
    }
    Some(v as u8)
}

/// If `host` is dotted IPv4, return octets; else None (needs DNS).
pub fn parse_ipv4_host(host: &[u8]) -> Option<[u8; 4]> {
    let mut parts = [0u8; 4];
    let mut n = 0usize;
    let mut start = 0usize;
    for (i, &b) in host.iter().enumerate() {
        if b == b'.' {
            if n >= 4 {
                return None;
            }
            parts[n] = parse_u8(&host[start..i])?;
            n += 1;
            start = i + 1;
        }
    }
    if n != 3 {
        return None;
    }
    parts[3] = parse_u8(&host[start..])?;
    Some(parts)
}

/// Strip spaces at ends.
fn trim(url: &[u8]) -> &[u8] {
    let mut i = 0;
    let mut j = url.len();
    while i < j && url[i] == b' ' {
        i += 1;
    }
    while j > i && url[j - 1] == b' ' {
        j -= 1;
    }
    &url[i..j]
}

/// Returns `None` if not `http://`, invalid, or `https://` (unsupported).
pub fn parse_http_url(url: &[u8]) -> Option<ParsedHttpUrl> {
    let url = trim(url);
    if url.len() >= 8 && eq_ci(url[0], b'h') && eq_ci(url[1], b't') {
        // https://
        if url.len() >= 8 && url[4] == b's' && url[5] == b':' {
            return None;
        }
    }
    let rest = starts_with_http(url)?;

    let mut host_end = rest.len();
    for (i, &b) in rest.iter().enumerate() {
        if b == b'/' {
            host_end = i;
            break;
        }
        if b == b':' {
            let ps = &rest[i + 1..];
            let mut pe = ps.len();
            for (j, &c) in ps.iter().enumerate() {
                if c == b'/' {
                    pe = j;
                    break;
                }
            }
            if pe == 0 {
                return None;
            }
            let mut p = 0u32;
            for &c in &ps[..pe] {
                if !c.is_ascii_digit() {
                    return None;
                }
                p = p * 10 + u32::from(c - b'0');
                if p > 65535 {
                    return None;
                }
            }
            let port = p as u16;
            let after = &rest[i + 1 + pe..];
            let path_src = if after.is_empty() {
                b"/" as &[u8]
            } else if after[0] == b'/' {
                after
            } else {
                return None;
            };
            return finish_parse(rest[..i].as_ref(), port, path_src);
        }
    }

    let host = &rest[..host_end];
    let path_src = if host_end >= rest.len() {
        b"/" as &[u8]
    } else {
        &rest[host_end..]
    };
    let path_src = if path_src.is_empty() {
        b"/" as &[u8]
    } else {
        path_src
    };
    finish_parse(host, 80, path_src)
}

fn finish_parse(host: &[u8], port: u16, path_src: &[u8]) -> Option<ParsedHttpUrl> {
    if host.is_empty() || host.len() > 95 || path_src.len() > 159 {
        return None;
    }

    let mut host_for_dns = [0u8; 96];
    host_for_dns[..host.len()].copy_from_slice(host);
    let mut host_header = [0u8; 96];
    let mut host_header_len = host.len();
    host_header[..host.len()].copy_from_slice(host);
    if port != 80 {
        // append :port
        let mut tmp = [0u8; 16];
        let mut n = port;
        let mut i = tmp.len();
        if n == 0 {
            i -= 1;
            tmp[i] = b'0';
        } else {
            while n > 0 && i > 0 {
                i -= 1;
                tmp[i] = b'0' + (n % 10) as u8;
                n /= 10;
            }
        }
        let ds = &tmp[i..];
        if host_header_len + 1 + ds.len() > 95 {
            return None;
        }
        host_header[host_header_len] = b':';
        host_header_len += 1;
        host_header[host_header_len..host_header_len + ds.len()].copy_from_slice(ds);
        host_header_len += ds.len();
    }

    let ip = if let Some(oct) = parse_ipv4_host(host) {
        oct
    } else {
        [0u8; 4]
    };
    let needs_dns = ip == [0u8; 4];

    let mut path = [0u8; 160];
    let path_len = path_src.len();
    path[..path_len].copy_from_slice(path_src);

    Some(ParsedHttpUrl {
        port,
        ip,
        needs_dns,
        host_for_dns,
        host_for_dns_len: host.len(),
        host_header,
        host_header_len,
        path,
        path_len,
    })
}
