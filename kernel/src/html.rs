// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! Strip and render a **subset** of HTML for the Eve browser: block tags, `<b>` / `<i>`,
//! simple `color` from `<style>` and inline `style="..."`, entities. **`<script>` is removed**
//! (not executed). **`<iframe>` / `<object>`** subtrees are skipped; **`<embed>`** open tags are
//! dropped; **`<meta>`, `<link>`, `<base>`** are ignored; **`javascript:` / `vbscript:` / `data:`**
//! in `<a href>` do not
//! open link styling. **No full CSS box model, no JavaScript engine** — pages work read-only.
//!
//! Render lines live in `static mut HTML_RENDER_LINES` so they are not embedded in `UiState` on
//! the stack (that ~14 KiB growth overflowed the bootloader stack and caused reboot loops).

#[derive(Clone, Copy)]
pub struct BrowserLine {
    pub data: [u8; BROWSER_LINE_CAP],
    pub len: usize,
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Default for BrowserLine {
    fn default() -> Self {
        Self {
            data: [0; BROWSER_LINE_CAP],
            len: 0,
            r: 0,
            g: 0,
            b: 0,
        }
    }
}

impl BrowserLine {
    const fn new(r: u8, g: u8, b: u8) -> Self {
        Self {
            data: [0; BROWSER_LINE_CAP],
            len: 0,
            r,
            g,
            b,
        }
    }

    fn clear_with(&mut self, r: u8, g: u8, b: u8) {
        self.len = 0;
        self.r = r;
        self.g = g;
        self.b = b;
    }
}

pub const BROWSER_LINE_CAP: usize = 96;
pub const BROWSER_MAX_LINES: usize = 128;

static mut HTML_RENDER_LINES: [BrowserLine; BROWSER_MAX_LINES] =
    [BrowserLine::new(0, 0, 0); BROWSER_MAX_LINES];
static mut STYLE_SCRATCH: [u8; 2048] = [0; 2048];
static mut DOM_TREE: crate::dom::DomTree = crate::dom::DomTree::new();

/// One rendered line for the browser view (read-only for compositor).
#[inline]
pub fn browser_line(i: usize) -> Option<&'static BrowserLine> {
    if i >= BROWSER_MAX_LINES {
        return None;
    }
    Some(unsafe { &HTML_RENDER_LINES[i] })
}

#[derive(Clone, Copy, Default)]
struct CssHints {
    body: (u8, u8, u8),
    h1: (u8, u8, u8),
    h2: (u8, u8, u8),
    link: (u8, u8, u8),
    has_body: bool,
    has_h1: bool,
    has_h2: bool,
}

#[inline]
fn to_lower(c: u8) -> u8 {
    if c >= b'A' && c <= b'Z' {
        c + 32
    } else {
        c
    }
}

fn starts_ci(s: &[u8], i: usize, pat: &[u8]) -> bool {
    if i.saturating_add(pat.len()) > s.len() {
        return false;
    }
    for j in 0..pat.len() {
        if to_lower(s[i + j]) != pat[j] {
            return false;
        }
    }
    true
}

fn find_sub_ci(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    for start in 0..=hay.len() - needle.len() {
        if starts_ci(hay, start, needle) {
            return Some(start);
        }
    }
    None
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn parse_hex_color(s: &[u8], i: &mut usize) -> Option<(u8, u8, u8)> {
    if *i >= s.len() || s[*i] != b'#' {
        return None;
    }
    *i += 1;
    let start = *i;
    while *i < s.len() && hex_val(s[*i]).is_some() {
        *i += 1;
    }
    let n = *i - start;
    if n == 6 {
        let r = hex_val(s[start])? * 16 + hex_val(s[start + 1])?;
        let g = hex_val(s[start + 2])? * 16 + hex_val(s[start + 3])?;
        let b = hex_val(s[start + 4])? * 16 + hex_val(s[start + 5])?;
        return Some((r, g, b));
    }
    if n == 3 {
        let r = hex_val(s[start])? * 17;
        let g = hex_val(s[start + 1])? * 17;
        let b = hex_val(s[start + 2])? * 17;
        return Some((r, g, b));
    }
    None
}

fn parse_named_color(s: &[u8], i: &mut usize) -> Option<(u8, u8, u8)> {
    const NAMES: &[(&[u8], (u8, u8, u8))] = &[
        (b"red", (0xcc, 0x22, 0x22)),
        (b"blue", (0x22, 0x44, 0xcc)),
        (b"green", (0x22, 0x88, 0x33)),
        (b"black", (0x11, 0x11, 0x11)),
        (b"white", (0xf0, 0xf0, 0xf0)),
        (b"gray", (0x66, 0x66, 0x66)),
        (b"grey", (0x66, 0x66, 0x66)),
        (b"purple", (0x66, 0x22, 0xaa)),
        (b"orange", (0xee, 0x66, 0x22)),
    ];
    for (name, rgb) in NAMES {
        if starts_ci(s, *i, name) {
            let end = *i + name.len();
            if end <= s.len()
                && (end == s.len()
                    || !matches!(s[end], b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9'))
            {
                *i = end;
                return Some(*rgb);
            }
        }
    }
    None
}

fn skip_ws(s: &[u8], i: &mut usize) {
    while *i < s.len() && matches!(s[*i], b' ' | b'\t' | b'\n' | b'\r') {
        *i += 1;
    }
}

fn parse_color_after_colon(s: &[u8], i: &mut usize) -> Option<(u8, u8, u8)> {
    skip_ws(s, i);
    if *i < s.len() && s[*i] == b'#' {
        return parse_hex_color(s, i);
    }
    parse_named_color(s, i)
}

fn scan_style_for_colors(css: &[u8], hints: &mut CssHints) {
    let mut i = 0usize;
    while i < css.len() {
        let win_end = (i + 192).min(css.len());
        let win = &css[i..win_end];
        let has_h1 = find_sub_ci(win, b"h1").is_some();
        let has_h2 = find_sub_ci(win, b"h2").is_some();
        let has_body = find_sub_ci(win, b"body").is_some();
        let has_a = find_sub_ci(win, b"a").is_some() || find_sub_ci(win, b"a ").is_some();
        if let Some(ci) = find_sub_ci(win, b"color") {
            let mut j = i + ci + 5;
            skip_ws(css, &mut j);
            if j < css.len() && css[j] == b':' {
                j += 1;
                if let Some(rgb) = parse_color_after_colon(css, &mut j) {
                    if has_h1 {
                        hints.h1 = rgb;
                        hints.has_h1 = true;
                    } else if has_h2 {
                        hints.h2 = rgb;
                        hints.has_h2 = true;
                    } else if has_body {
                        hints.body = rgb;
                        hints.has_body = true;
                    } else if has_a {
                        hints.link = rgb;
                    }
                }
            }
        }
        i += 32;
    }
}

fn parse_inline_style_color(style: &[u8]) -> Option<(u8, u8, u8)> {
    let pos = find_sub_ci(style, b"color")?;
    let mut j = pos + 5;
    skip_ws(style, &mut j);
    if j >= style.len() || style[j] != b':' {
        return None;
    }
    j += 1;
    parse_color_after_colon(style, &mut j)
}

fn parse_inline_style_display_none(style: &[u8]) -> bool {
    let Some(pos) = find_sub_ci(style, b"display") else {
        return false;
    };
    let mut j = pos + 7;
    skip_ws(style, &mut j);
    if j >= style.len() || style[j] != b':' {
        return false;
    }
    j += 1;
    skip_ws(style, &mut j);
    starts_ci(style, j, b"none")
}

fn parse_inline_style_left_indent(style: &[u8]) -> u8 {
    let mut best = 0u8;
    for key in [b"padding-left".as_slice(), b"margin-left".as_slice()] {
        let Some(pos) = find_sub_ci(style, key) else {
            continue;
        };
        let mut j = pos + key.len();
        skip_ws(style, &mut j);
        if j >= style.len() || style[j] != b':' {
            continue;
        }
        j += 1;
        skip_ws(style, &mut j);
        let mut v: u16 = 0;
        let mut any = false;
        while j < style.len() && style[j].is_ascii_digit() {
            any = true;
            v = v
                .saturating_mul(10)
                .saturating_add(u16::from(style[j] - b'0'));
            j += 1;
        }
        if !any {
            continue;
        }
        let spaces = if v >= 64 {
            8
        } else if v >= 48 {
            6
        } else if v >= 32 {
            4
        } else if v >= 16 {
            2
        } else {
            0
        };
        best = best.max(spaces);
    }
    best
}

fn looks_like_html(raw: &[u8]) -> bool {
    let n = raw.len().min(512);
    let head = &raw[..n];
    find_sub_ci(head, b"<!doctype html").is_some()
        || find_sub_ci(head, b"<html").is_some()
        || find_sub_ci(head, b"<head").is_some()
        || find_sub_ci(head, b"<body").is_some()
        || find_sub_ci(head, b"<div").is_some()
        || find_sub_ci(head, b"<p>").is_some()
        || find_sub_ci(head, b"<br").is_some()
}

fn flush_line(
    lines: &mut [BrowserLine; BROWSER_MAX_LINES],
    count: &mut usize,
    cur: &mut BrowserLine,
    trunc: &mut bool,
) {
    if *count >= BROWSER_MAX_LINES {
        *trunc = true;
        cur.len = 0;
        return;
    }
    lines[*count] = *cur;
    *count += 1;
    cur.len = 0;
}

fn emit_break(
    lines: &mut [BrowserLine; BROWSER_MAX_LINES],
    count: &mut usize,
    cur: &mut BrowserLine,
    trunc: &mut bool,
    blank_after: bool,
    fg: (u8, u8, u8),
) {
    if cur.len > 0 {
        flush_line(lines, count, cur, trunc);
    }
    if blank_after && *count < BROWSER_MAX_LINES {
        lines[*count] = BrowserLine::new(fg.0, fg.1, fg.2);
        *count += 1;
    }
    cur.clear_with(fg.0, fg.1, fg.2);
}

fn emit_char(
    lines: &mut [BrowserLine; BROWSER_MAX_LINES],
    count: &mut usize,
    cur: &mut BrowserLine,
    trunc: &mut bool,
    c: u8,
    fg: (u8, u8, u8),
) {
    if cur.len >= BROWSER_LINE_CAP {
        flush_line(lines, count, cur, trunc);
        cur.clear_with(fg.0, fg.1, fg.2);
    }
    if *count >= BROWSER_MAX_LINES && cur.len >= BROWSER_LINE_CAP {
        *trunc = true;
        return;
    }
    if cur.len < BROWSER_LINE_CAP {
        cur.data[cur.len] = c;
        cur.len += 1;
    }
}

fn plain_lines(
    raw: &[u8],
    lines: &mut [BrowserLine; BROWSER_MAX_LINES],
    count: &mut usize,
    trunc: &mut bool,
) {
    let fg = (0x22u8, 0x22u8, 0x22u8);
    let mut cur = BrowserLine::new(fg.0, fg.1, fg.2);
    for &b in raw {
        if b == b'\n' || b == b'\r' {
            if cur.len > 0 {
                flush_line(lines, count, &mut cur, trunc);
            }
            cur.clear_with(fg.0, fg.1, fg.2);
            continue;
        }
        if *trunc {
            break;
        }
        emit_char(lines, count, &mut cur, trunc, b, fg);
    }
    if cur.len > 0 && !*trunc {
        flush_line(lines, count, &mut cur, trunc);
    }
}

fn skip_until_gt(s: &[u8], i: &mut usize) {
    while *i < s.len() && s[*i] != b'>' {
        *i += 1;
    }
    if *i < s.len() {
        *i += 1;
    }
}

/// `href` must be a token boundary (not `hreflang`, etc.).
fn tag_href_value_dangerous(tag: &[u8]) -> bool {
    let Some(hi) = find_sub_ci(tag, b"href") else {
        return false;
    };
    let after = hi + 4;
    if after < tag.len()
        && matches!(tag[after], b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_')
    {
        return false;
    }
    let mut j = after;
    skip_ws(tag, &mut j);
    if j >= tag.len() || tag[j] != b'=' {
        return false;
    }
    j += 1;
    skip_ws(tag, &mut j);
    if j >= tag.len() {
        return false;
    }
    let val = if tag[j] == b'"' || tag[j] == b'\'' {
        let q = tag[j];
        j += 1;
        let start = j;
        while j < tag.len() && tag[j] != q {
            j += 1;
        }
        &tag[start..j]
    } else {
        let start = j;
        while j < tag.len() && !matches!(tag[j], b' ' | b'\t' | b'\r' | b'\n' | b'>') {
            j += 1;
        }
        &tag[start..j]
    };
    starts_ci(val, 0, b"javascript:")
        || starts_ci(val, 0, b"vbscript:")
        || starts_ci(val, 0, b"data:")
}

/// Fill `HTML_RENDER_LINES` / `line_count` from HTTP body bytes. Merges visual truncation with `inet.page_truncated` in caller.
#[allow(static_mut_refs)] // Single-threaded kernel; no concurrent access to `HTML_RENDER_LINES`.
pub fn format_document(
    raw: &[u8],
    line_count: &mut usize,
    html_truncated: &mut bool,
    scripts_stripped: &mut bool,
) {
    *line_count = 0;
    *html_truncated = false;
    *scripts_stripped = false;

    let lines = unsafe { &mut HTML_RENDER_LINES };
    // Keep a lightweight DOM skeleton in sync with visible content parsing.
    crate::dom::build_text_dom(raw, unsafe { &mut DOM_TREE });

    if !looks_like_html(raw) {
        plain_lines(raw, lines, line_count, html_truncated);
        return;
    }

    let mut hints = CssHints {
        body: (0x22, 0x22, 0x22),
        h1: (0x00, 0x40, 0x90),
        h2: (0x10, 0x60, 0x70),
        link: (0x22, 0x22, 0xcc),
        has_body: false,
        has_h1: false,
        has_h2: false,
    };

    let mut i = 0usize;
    let mut in_script = false;
    let mut in_noscript = false;
    let mut in_iframe = false;
    let mut in_object = false;
    let mut in_title = false;
    let mut in_style = false;
    let mut a_styled = false;
    let mut list_depth: u8 = 0;
    let style_buf = unsafe { &mut STYLE_SCRATCH[..] };
    let mut style_len = 0usize;

    let mut default_fg = hints.body;
    let mut cur = BrowserLine::new(default_fg.0, default_fg.1, default_fg.2);
    let mut fg_stack = [(0u8, 0u8, 0u8); 12];
    let mut fg_sp = 0usize;
    let mut cur_fg = default_fg;

    macro_rules! set_fg {
        ($rgb:expr) => {{
            cur_fg = $rgb;
            cur.r = cur_fg.0;
            cur.g = cur_fg.1;
            cur.b = cur_fg.2;
        }};
    }

    while i < raw.len() {
        if *line_count >= BROWSER_MAX_LINES {
            *html_truncated = true;
            break;
        }

        if in_script {
            if let Some(rel) = find_sub_ci(&raw[i..], b"</script>") {
                i += rel + 9;
                in_script = false;
            } else {
                break;
            }
            continue;
        }
        if in_noscript {
            if let Some(rel) = find_sub_ci(&raw[i..], b"</noscript>") {
                i += rel + 11;
                in_noscript = false;
            } else {
                break;
            }
            continue;
        }
        if in_iframe {
            if let Some(rel) = find_sub_ci(&raw[i..], b"</iframe>") {
                i += rel + 9;
                in_iframe = false;
            } else {
                break;
            }
            continue;
        }
        if in_object {
            if let Some(rel) = find_sub_ci(&raw[i..], b"</object>") {
                i += rel + 9;
                in_object = false;
            } else {
                break;
            }
            continue;
        }
        if in_title {
            if let Some(rel) = find_sub_ci(&raw[i..], b"</title>") {
                i += rel + 8;
                in_title = false;
            } else {
                break;
            }
            continue;
        }
        if in_style {
            if let Some(rel) = find_sub_ci(&raw[i..], b"</style>") {
                if style_len > 0 {
                    scan_style_for_colors(&style_buf[..style_len], &mut hints);
                    if hints.has_body {
                        default_fg = hints.body;
                        if fg_sp == 0 {
                            set_fg!(default_fg);
                        }
                    }
                }
                style_len = 0;
                i += rel + 8;
                in_style = false;
            } else {
                let take = (raw.len() - i).min(style_buf.len().saturating_sub(style_len));
                if take > 0 {
                    style_buf[style_len..style_len + take].copy_from_slice(&raw[i..i + take]);
                    style_len += take;
                    i += take;
                } else {
                    i = raw.len();
                }
            }
            continue;
        }

        if raw[i] != b'<' {
            if raw[i] == b'&' {
                let rest = &raw[i..];
                if starts_ci(rest, 0, b"&nbsp;") {
                    emit_char(
                        lines,
                        line_count,
                        &mut cur,
                        html_truncated,
                        b' ',
                        cur_fg,
                    );
                    i += 6;
                    continue;
                }
                if starts_ci(rest, 0, b"&amp;") {
                    emit_char(lines, line_count, &mut cur, html_truncated, b'&', cur_fg);
                    i += 5;
                    continue;
                }
                if starts_ci(rest, 0, b"&lt;") {
                    emit_char(lines, line_count, &mut cur, html_truncated, b'<', cur_fg);
                    i += 4;
                    continue;
                }
                if starts_ci(rest, 0, b"&gt;") {
                    emit_char(lines, line_count, &mut cur, html_truncated, b'>', cur_fg);
                    i += 4;
                    continue;
                }
                if starts_ci(rest, 0, b"&quot;") {
                    emit_char(lines, line_count, &mut cur, html_truncated, b'"', cur_fg);
                    i += 6;
                    continue;
                }
            }
            let ch = raw[i];
            if ch == b'\n' || ch == b'\r' {
                i += 1;
                continue;
            }
            if ch == b'\t' || ch == b' ' {
                if cur.len > 0 && cur.data[cur.len - 1] != b' ' {
                    emit_char(lines, line_count, &mut cur, html_truncated, b' ', cur_fg);
                }
                i += 1;
                continue;
            }
            emit_char(lines, line_count, &mut cur, html_truncated, ch, cur_fg);
            i += 1;
            continue;
        }

        // Tag
        if starts_ci(raw, i, b"<!--") {
            i += 4;
            while i + 2 < raw.len() && !(raw[i] == b'-' && raw[i + 1] == b'-' && raw[i + 2] == b'>') {
                i += 1;
            }
            i = (i + 3).min(raw.len());
            continue;
        }

        if starts_ci(raw, i, b"<meta") {
            skip_until_gt(raw, &mut i);
            continue;
        }
        if starts_ci(raw, i, b"<link") {
            skip_until_gt(raw, &mut i);
            continue;
        }
        if starts_ci(raw, i, b"<base") {
            skip_until_gt(raw, &mut i);
            continue;
        }

        if starts_ci(raw, i, b"<script") {
            *scripts_stripped = true;
            in_script = true;
            skip_until_gt(raw, &mut i);
            continue;
        }
        if starts_ci(raw, i, b"<iframe") {
            in_iframe = true;
            skip_until_gt(raw, &mut i);
            continue;
        }
        if starts_ci(raw, i, b"<object") {
            in_object = true;
            skip_until_gt(raw, &mut i);
            continue;
        }
        if starts_ci(raw, i, b"<embed") {
            skip_until_gt(raw, &mut i);
            continue;
        }
        if starts_ci(raw, i, b"<style") {
            in_style = true;
            style_len = 0;
            skip_until_gt(raw, &mut i);
            continue;
        }
        if starts_ci(raw, i, b"<title") {
            in_title = true;
            skip_until_gt(raw, &mut i);
            continue;
        }
        if starts_ci(raw, i, b"<noscript") {
            in_noscript = true;
            skip_until_gt(raw, &mut i);
            continue;
        }

        let mut tag_end = i + 1;
        while tag_end < raw.len() && raw[tag_end] != b'>' {
            tag_end += 1;
        }
        if tag_end >= raw.len() {
            break;
        }
        let tag_slice = &raw[i..tag_end];
        i = tag_end + 1;

        if tag_slice.len() < 2 {
            continue;
        }
        let is_close = tag_slice[1] == b'/';
        let mut name_start = if is_close { 2 } else { 1 };
        while name_start < tag_slice.len()
            && matches!(tag_slice[name_start], b' ' | b'\t' | b'\n' | b'/')
        {
            name_start += 1;
        }
        let mut name_end = name_start;
        while name_end < tag_slice.len()
            && matches!(
                tag_slice[name_end],
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9'
            )
        {
            name_end += 1;
        }
        let name = &tag_slice[name_start..name_end];

        let inline_color = if !is_close {
            if let Some(si) = find_sub_ci(tag_slice, b"style=\"") {
                let q0 = si + 7;
                let mut q1 = q0;
                while q1 < tag_slice.len() && tag_slice[q1] != b'"' {
                    q1 += 1;
                }
                if q1 > q0 {
                    parse_inline_style_color(&tag_slice[q0..q1])
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        let inline_hidden = if !is_close {
            if let Some(si) = find_sub_ci(tag_slice, b"style=\"") {
                let q0 = si + 7;
                let mut q1 = q0;
                while q1 < tag_slice.len() && tag_slice[q1] != b'"' {
                    q1 += 1;
                }
                if q1 > q0 {
                    parse_inline_style_display_none(&tag_slice[q0..q1])
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };
        let inline_indent = if !is_close {
            if let Some(si) = find_sub_ci(tag_slice, b"style=\"") {
                let q0 = si + 7;
                let mut q1 = q0;
                while q1 < tag_slice.len() && tag_slice[q1] != b'"' {
                    q1 += 1;
                }
                if q1 > q0 {
                    parse_inline_style_left_indent(&tag_slice[q0..q1])
                } else {
                    0
                }
            } else {
                0
            }
        } else {
            0
        };

        let name_is = |n: &[u8]| name.len() == n.len() && starts_ci(name, 0, n);

        if is_close {
            if name_is(b"ul") || name_is(b"ol") {
                list_depth = list_depth.saturating_sub(1);
            }
            if name_is(b"a") {
                if a_styled {
                    if fg_sp > 0 {
                        fg_sp -= 1;
                        set_fg!(fg_stack[fg_sp]);
                    } else {
                        set_fg!(default_fg);
                    }
                    a_styled = false;
                }
                continue;
            }
            if name_is(b"b") || name_is(b"strong") || name_is(b"i") || name_is(b"em") {
                if fg_sp > 0 {
                    fg_sp -= 1;
                    set_fg!(fg_stack[fg_sp]);
                } else {
                    set_fg!(default_fg);
                }
            }
            if name_is(b"p") || name_is(b"div") || name_is(b"li") || name_is(b"tr")
                || name_is(b"h1") || name_is(b"h2") || name_is(b"h3")
            {
                emit_break(
                    lines,
                    line_count,
                    &mut cur,
                    html_truncated,
                    name_is(b"p") || name_is(b"div"),
                    default_fg,
                );
                set_fg!(default_fg);
            }
            continue;
        }

        // Open / empty tags
        if inline_hidden {
            continue;
        }
        if name_is(b"ul") || name_is(b"ol") {
            list_depth = list_depth.saturating_add(1);
            emit_break(
                lines,
                line_count,
                &mut cur,
                html_truncated,
                false,
                default_fg,
            );
            continue;
        }
        if name_is(b"br") || name_is(b"hr") {
            emit_break(
                lines,
                line_count,
                &mut cur,
                html_truncated,
                false,
                default_fg,
            );
            set_fg!(default_fg);
            continue;
        }

        if name_is(b"h1") {
            let h = if hints.has_h1 {
                hints.h1
            } else {
                (0x00, 0x40, 0x90)
            };
            emit_break(lines, line_count, &mut cur, html_truncated, true, h);
            if let Some(rgb) = inline_color {
                set_fg!(rgb);
            } else {
                set_fg!(h);
            }
            continue;
        }
        if name_is(b"h2") {
            let h = if hints.has_h2 {
                hints.h2
            } else {
                (0x10, 0x60, 0x70)
            };
            emit_break(lines, line_count, &mut cur, html_truncated, true, h);
            if let Some(rgb) = inline_color {
                set_fg!(rgb);
            } else {
                set_fg!(h);
            }
            continue;
        }
        if name_is(b"h3") {
            emit_break(
                lines,
                line_count,
                &mut cur,
                html_truncated,
                true,
                (0x44, 0x44, 0x88),
            );
            if let Some(rgb) = inline_color {
                set_fg!(rgb);
            } else {
                set_fg!((0x44, 0x44, 0x88));
            }
            continue;
        }

        if name_is(b"b") || name_is(b"strong") {
            let bfg = (0x11, 0x11, 0x11);
            if fg_sp < fg_stack.len() {
                fg_stack[fg_sp] = cur_fg;
                fg_sp += 1;
            }
            set_fg!(bfg);
            continue;
        }
        if name_is(b"i") || name_is(b"em") {
            let ifg = (0x55, 0x44, 0x77);
            if fg_sp < fg_stack.len() {
                fg_stack[fg_sp] = cur_fg;
                fg_sp += 1;
            }
            set_fg!(ifg);
            continue;
        }
        if name_is(b"a") {
            if tag_href_value_dangerous(tag_slice) {
                continue;
            }
            a_styled = true;
            let l = hints.link;
            if fg_sp < fg_stack.len() {
                fg_stack[fg_sp] = cur_fg;
                fg_sp += 1;
            }
            if let Some(rgb) = inline_color {
                set_fg!(rgb);
            } else {
                set_fg!(l);
            }
            continue;
        }

        if name_is(b"p") || name_is(b"div") || name_is(b"tr") {
            emit_break(
                lines,
                line_count,
                &mut cur,
                html_truncated,
                true,
                default_fg,
            );
            set_fg!(default_fg);
            continue;
        }
        if name_is(b"li") {
            let indent = usize::from(list_depth.saturating_mul(2)).saturating_add(usize::from(inline_indent));
            for _ in 0..indent.min(16) {
                emit_char(lines, line_count, &mut cur, html_truncated, b' ', cur_fg);
            }
            emit_char(lines, line_count, &mut cur, html_truncated, b'-', cur_fg);
            emit_char(lines, line_count, &mut cur, html_truncated, b' ', cur_fg);
            continue;
        }
    }

    if cur.len > 0 && *line_count < BROWSER_MAX_LINES {
        flush_line(lines, line_count, &mut cur, html_truncated);
    }

    if *scripts_stripped && *line_count < BROWSER_MAX_LINES {
        let mut note = BrowserLine::new(0x88, 0x55, 0x22);
        let msg = b"[JS NOT EXECUTED IN EVE]";
        let n = msg.len().min(BROWSER_LINE_CAP);
        note.data[..n].copy_from_slice(&msg[..n]);
        note.len = n;
        lines[*line_count] = note;
        *line_count += 1;
    }
}
