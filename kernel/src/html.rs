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

#[derive(Clone, Copy, PartialEq, Eq)]
enum DisplayHint {
    Inherit,
    Block,
    Inline,
    None,
}

#[derive(Clone, Copy)]
struct BoxStyle {
    color: Option<(u8, u8, u8)>,
    display: DisplayHint,
    margin_left: u8,
    padding_left: u8,
    margin_top: u8,
    margin_bottom: u8,
    padding_top: u8,
    padding_bottom: u8,
    width_chars: Option<u8>,
}

impl BoxStyle {
    const fn empty() -> Self {
        Self {
            color: None,
            display: DisplayHint::Inherit,
            margin_left: 0,
            padding_left: 0,
            margin_top: 0,
            margin_bottom: 0,
            padding_top: 0,
            padding_bottom: 0,
            width_chars: None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SelectorKind {
    Tag,
    Class,
    Id,
}

#[derive(Clone, Copy)]
struct StyleRule {
    kind: SelectorKind,
    name: [u8; 24],
    name_len: u8,
    style: BoxStyle,
}

impl StyleRule {
    const fn empty() -> Self {
        Self {
            kind: SelectorKind::Tag,
            name: [0; 24],
            name_len: 0,
            style: BoxStyle::empty(),
        }
    }
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

fn parse_css_number_unit(s: &[u8], i: &mut usize) -> Option<(u16, u8)> {
    skip_ws(s, i);
    let mut v: u16 = 0;
    let mut any = false;
    while *i < s.len() && s[*i].is_ascii_digit() {
        any = true;
        v = v
            .saturating_mul(10)
            .saturating_add(u16::from(s[*i] - b'0'));
        *i += 1;
    }
    if !any {
        return None;
    }
    if starts_ci(s, *i, b"px") {
        *i += 2;
        return Some((v, b'p'));
    }
    if starts_ci(s, *i, b"ch") {
        *i += 2;
        return Some((v, b'c'));
    }
    if *i < s.len() && s[*i] == b'%' {
        *i += 1;
        return Some((v, b'%'));
    }
    Some((v, b'n'))
}

fn px_to_spaces(px: u16) -> u8 {
    if px >= 64 {
        8
    } else if px >= 48 {
        6
    } else if px >= 32 {
        4
    } else if px >= 16 {
        2
    } else {
        0
    }
}

fn px_to_blank_lines(px: u16) -> u8 {
    if px >= 40 {
        2
    } else if px >= 16 {
        1
    } else {
        0
    }
}

fn parse_inline_box_style(style: &[u8], line_cap: usize) -> BoxStyle {
    let mut out = BoxStyle::empty();
    let mut i = 0usize;
    while i < style.len() {
        skip_ws(style, &mut i);
        let key_start = i;
        while i < style.len() && matches!(style[i], b'a'..=b'z' | b'A'..=b'Z' | b'-') {
            i += 1;
        }
        let key = &style[key_start..i];
        skip_ws(style, &mut i);
        if i >= style.len() || style[i] != b':' {
            while i < style.len() && style[i] != b';' {
                i += 1;
            }
            i = i.saturating_add(1);
            continue;
        }
        i += 1;
        skip_ws(style, &mut i);

        if key.len() == 5 && starts_ci(key, 0, b"color") {
            out.color = parse_color_after_colon(style, &mut i);
        } else if key.len() == 7 && starts_ci(key, 0, b"display") {
            if starts_ci(style, i, b"none") {
                out.display = DisplayHint::None;
                i += 4;
            } else if starts_ci(style, i, b"block") {
                out.display = DisplayHint::Block;
                i += 5;
            } else if starts_ci(style, i, b"inline") {
                out.display = DisplayHint::Inline;
                i += 6;
            }
        } else if starts_ci(key, 0, b"margin-left") {
            if let Some((v, _)) = parse_css_number_unit(style, &mut i) {
                out.margin_left = px_to_spaces(v);
            }
        } else if starts_ci(key, 0, b"padding-left") {
            if let Some((v, _)) = parse_css_number_unit(style, &mut i) {
                out.padding_left = px_to_spaces(v);
            }
        } else if starts_ci(key, 0, b"margin-top") {
            if let Some((v, _)) = parse_css_number_unit(style, &mut i) {
                out.margin_top = px_to_blank_lines(v);
            }
        } else if starts_ci(key, 0, b"margin-bottom") {
            if let Some((v, _)) = parse_css_number_unit(style, &mut i) {
                out.margin_bottom = px_to_blank_lines(v);
            }
        } else if starts_ci(key, 0, b"padding-top") {
            if let Some((v, _)) = parse_css_number_unit(style, &mut i) {
                out.padding_top = px_to_blank_lines(v);
            }
        } else if starts_ci(key, 0, b"padding-bottom") {
            if let Some((v, _)) = parse_css_number_unit(style, &mut i) {
                out.padding_bottom = px_to_blank_lines(v);
            }
        } else if key.len() == 5 && starts_ci(key, 0, b"width") {
            if let Some((v, unit)) = parse_css_number_unit(style, &mut i) {
                let w = match unit {
                    b'c' => v,
                    b'%' => ((line_cap as u16).saturating_mul(v) / 100).max(8),
                    _ => (v / 6).max(8),
                };
                out.width_chars = Some(w.min(BROWSER_LINE_CAP as u16) as u8);
            }
        }
        while i < style.len() && style[i] != b';' {
            i += 1;
        }
        i = i.saturating_add(1);
    }
    out
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

fn extract_attr_value<'a>(tag: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let pos = find_sub_ci(tag, key)?;
    let mut j = pos + key.len();
    skip_ws(tag, &mut j);
    if j >= tag.len() || tag[j] != b'=' {
        return None;
    }
    j += 1;
    skip_ws(tag, &mut j);
    if j >= tag.len() {
        return None;
    }
    if tag[j] == b'"' || tag[j] == b'\'' {
        let q = tag[j];
        j += 1;
        let start = j;
        while j < tag.len() && tag[j] != q {
            j += 1;
        }
        return Some(&tag[start..j]);
    }
    let start = j;
    while j < tag.len() && !matches!(tag[j], b' ' | b'\t' | b'\r' | b'\n' | b'>') {
        j += 1;
    }
    Some(&tag[start..j])
}

fn copy_name_lower(dst: &mut [u8], src: &[u8]) -> usize {
    let n = src.len().min(dst.len());
    for i in 0..n {
        dst[i] = to_lower(src[i]);
    }
    n
}

fn scan_style_rules(css: &[u8], rules: &mut [StyleRule], rule_count: &mut usize, line_cap: usize) {
    let mut i = 0usize;
    while i < css.len() && *rule_count < rules.len() {
        skip_ws(css, &mut i);
        if i >= css.len() {
            break;
        }
        let selector_start = i;
        while i < css.len() && css[i] != b'{' {
            i += 1;
        }
        if i >= css.len() {
            break;
        }
        let selector = &css[selector_start..i];
        i += 1;
        let decl_start = i;
        while i < css.len() && css[i] != b'}' {
            i += 1;
        }
        let decl = &css[decl_start..i.min(css.len())];
        if i < css.len() {
            i += 1;
        }

        let mut sel_i = 0usize;
        skip_ws(selector, &mut sel_i);
        if sel_i >= selector.len() {
            continue;
        }
        let mut rule = StyleRule::empty();
        if selector[sel_i] == b'.' {
            rule.kind = SelectorKind::Class;
            sel_i += 1;
        } else if selector[sel_i] == b'#' {
            rule.kind = SelectorKind::Id;
            sel_i += 1;
        } else {
            rule.kind = SelectorKind::Tag;
        }
        let name_start = sel_i;
        while sel_i < selector.len()
            && matches!(selector[sel_i], b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_')
        {
            sel_i += 1;
        }
        let name = &selector[name_start..sel_i];
        if name.is_empty() {
            continue;
        }
        rule.name_len = copy_name_lower(&mut rule.name, name) as u8;
        rule.style = parse_inline_box_style(decl, line_cap);
        rules[*rule_count] = rule;
        *rule_count += 1;
    }
}

fn class_matches(class_attr: &[u8], name: &[u8]) -> bool {
    let mut i = 0usize;
    while i < class_attr.len() {
        while i < class_attr.len() && class_attr[i].is_ascii_whitespace() {
            i += 1;
        }
        let start = i;
        while i < class_attr.len() && !class_attr[i].is_ascii_whitespace() {
            i += 1;
        }
        let token = &class_attr[start..i];
        if token.len() == name.len() {
            let mut same = true;
            for k in 0..name.len() {
                if to_lower(token[k]) != name[k] {
                    same = false;
                    break;
                }
            }
            if same {
                return true;
            }
        }
    }
    false
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

fn emit_char_cap(
    lines: &mut [BrowserLine; BROWSER_MAX_LINES],
    count: &mut usize,
    cur: &mut BrowserLine,
    trunc: &mut bool,
    c: u8,
    fg: (u8, u8, u8),
    cap: usize,
) {
    let limit = cap.clamp(1, BROWSER_LINE_CAP);
    if cur.len >= limit {
        flush_line(lines, count, cur, trunc);
        cur.clear_with(fg.0, fg.1, fg.2);
    }
    if cur.len < limit {
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
    let mut style_rules = [StyleRule::empty(); 32];
    let mut style_rule_count = 0usize;
    let mut block_indent_stack = [0u8; 16];
    let mut block_width_stack = [BROWSER_LINE_CAP as u8; 16];
    let mut block_sp = 0usize;

    let mut default_fg = hints.body;
    let mut cur = BrowserLine::new(default_fg.0, default_fg.1, default_fg.2);
    let mut fg_stack = [(0u8, 0u8, 0u8); 12];
    let mut fg_sp = 0usize;
    let mut cur_fg = default_fg;
    let mut active_indent = 0u8;
    let mut active_width = BROWSER_LINE_CAP as u8;

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
                    style_rule_count = 0;
                    scan_style_rules(
                        &style_buf[..style_len],
                        &mut style_rules,
                        &mut style_rule_count,
                        BROWSER_LINE_CAP,
                    );
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
                    if cur.len == 0 {
                        for _ in 0..usize::from(active_indent).min(24) {
                            emit_char_cap(
                                lines,
                                line_count,
                                &mut cur,
                                html_truncated,
                                b' ',
                                cur_fg,
                                usize::from(active_width),
                            );
                        }
                    }
                    emit_char_cap(
                        lines,
                        line_count,
                        &mut cur,
                        html_truncated,
                        b' ',
                        cur_fg,
                        usize::from(active_width),
                    );
                    i += 6;
                    continue;
                }
                if starts_ci(rest, 0, b"&amp;") {
                    emit_char_cap(
                        lines,
                        line_count,
                        &mut cur,
                        html_truncated,
                        b'&',
                        cur_fg,
                        usize::from(active_width),
                    );
                    i += 5;
                    continue;
                }
                if starts_ci(rest, 0, b"&lt;") {
                    emit_char_cap(
                        lines,
                        line_count,
                        &mut cur,
                        html_truncated,
                        b'<',
                        cur_fg,
                        usize::from(active_width),
                    );
                    i += 4;
                    continue;
                }
                if starts_ci(rest, 0, b"&gt;") {
                    emit_char_cap(
                        lines,
                        line_count,
                        &mut cur,
                        html_truncated,
                        b'>',
                        cur_fg,
                        usize::from(active_width),
                    );
                    i += 4;
                    continue;
                }
                if starts_ci(rest, 0, b"&quot;") {
                    emit_char_cap(
                        lines,
                        line_count,
                        &mut cur,
                        html_truncated,
                        b'"',
                        cur_fg,
                        usize::from(active_width),
                    );
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
                    emit_char_cap(
                        lines,
                        line_count,
                        &mut cur,
                        html_truncated,
                        b' ',
                        cur_fg,
                        usize::from(active_width),
                    );
                }
                i += 1;
                continue;
            }
            if cur.len == 0 {
                for _ in 0..usize::from(active_indent).min(24) {
                    emit_char_cap(
                        lines,
                        line_count,
                        &mut cur,
                        html_truncated,
                        b' ',
                        cur_fg,
                        usize::from(active_width),
                    );
                }
            }
            emit_char_cap(
                lines,
                line_count,
                &mut cur,
                html_truncated,
                ch,
                cur_fg,
                usize::from(active_width),
            );
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

        let name_is = |n: &[u8]| name.len() == n.len() && starts_ci(name, 0, n);
        let class_attr = if is_close {
            None
        } else {
            extract_attr_value(tag_slice, b"class")
        };
        let id_attr = if is_close {
            None
        } else {
            extract_attr_value(tag_slice, b"id")
        };
        let inline_style = if is_close {
            None
        } else {
            extract_attr_value(tag_slice, b"style")
        };
        let mut computed = BoxStyle::empty();
        if !is_close {
            for rule in style_rules.iter().take(style_rule_count) {
                let rn = &rule.name[..usize::from(rule.name_len)];
                let matched = match rule.kind {
                    SelectorKind::Tag => name.len() == rn.len() && starts_ci(name, 0, rn),
                    SelectorKind::Class => class_attr.map(|c| class_matches(c, rn)).unwrap_or(false),
                    SelectorKind::Id => id_attr
                        .map(|id| id.len() == rn.len() && starts_ci(id, 0, rn))
                        .unwrap_or(false),
                };
                if matched {
                    if let Some(c) = rule.style.color {
                        computed.color = Some(c);
                    }
                    if rule.style.display != DisplayHint::Inherit {
                        computed.display = rule.style.display;
                    }
                    computed.margin_left = computed.margin_left.max(rule.style.margin_left);
                    computed.padding_left = computed.padding_left.max(rule.style.padding_left);
                    computed.margin_top = computed.margin_top.max(rule.style.margin_top);
                    computed.margin_bottom = computed.margin_bottom.max(rule.style.margin_bottom);
                    computed.padding_top = computed.padding_top.max(rule.style.padding_top);
                    computed.padding_bottom = computed.padding_bottom.max(rule.style.padding_bottom);
                    if let Some(w) = rule.style.width_chars {
                        computed.width_chars = Some(w);
                    }
                }
            }
            if let Some(st) = inline_style {
                let inl = parse_inline_box_style(st, BROWSER_LINE_CAP);
                if let Some(c) = inl.color {
                    computed.color = Some(c);
                }
                if inl.display != DisplayHint::Inherit {
                    computed.display = inl.display;
                }
                computed.margin_left = computed.margin_left.max(inl.margin_left);
                computed.padding_left = computed.padding_left.max(inl.padding_left);
                computed.margin_top = computed.margin_top.max(inl.margin_top);
                computed.margin_bottom = computed.margin_bottom.max(inl.margin_bottom);
                computed.padding_top = computed.padding_top.max(inl.padding_top);
                computed.padding_bottom = computed.padding_bottom.max(inl.padding_bottom);
                if let Some(w) = inl.width_chars {
                    computed.width_chars = Some(w);
                }
            }
        }
        let inline_color = computed.color;
        let inline_hidden = computed.display == DisplayHint::None;
        let inline_indent = computed.margin_left.saturating_add(computed.padding_left);

        if is_close {
            if name_is(b"ul") || name_is(b"ol") {
                list_depth = list_depth.saturating_sub(1);
            }
            if name_is(b"p")
                || name_is(b"div")
                || name_is(b"li")
                || name_is(b"tr")
                || name_is(b"h1")
                || name_is(b"h2")
                || name_is(b"h3")
                || name_is(b"section")
                || name_is(b"article")
                || name_is(b"main")
                || name_is(b"nav")
                || name_is(b"header")
                || name_is(b"footer")
                || name_is(b"blockquote")
            {
                if block_sp > 0 {
                    block_sp -= 1;
                    active_indent = block_indent_stack[block_sp];
                    active_width = block_width_stack[block_sp];
                } else {
                    active_indent = 0;
                    active_width = BROWSER_LINE_CAP as u8;
                }
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

        if name_is(b"p")
            || name_is(b"div")
            || name_is(b"tr")
            || name_is(b"section")
            || name_is(b"article")
            || name_is(b"main")
            || name_is(b"nav")
            || name_is(b"header")
            || name_is(b"footer")
            || name_is(b"blockquote")
        {
            for _ in 0..usize::from(computed.margin_top.saturating_add(computed.padding_top)).min(3) {
                emit_break(lines, line_count, &mut cur, html_truncated, true, default_fg);
            }
            emit_break(
                lines,
                line_count,
                &mut cur,
                html_truncated,
                true,
                default_fg,
            );
            if block_sp < block_indent_stack.len() {
                block_indent_stack[block_sp] = active_indent;
                block_width_stack[block_sp] = active_width;
                block_sp += 1;
            }
            active_indent = active_indent
                .saturating_add(inline_indent)
                .saturating_add(list_depth.saturating_mul(2));
            if let Some(w) = computed.width_chars {
                active_width = active_width.min(w.max(8));
            }
            set_fg!(default_fg);
            continue;
        }
        if name_is(b"li") {
            for _ in 0..usize::from(computed.margin_top.saturating_add(computed.padding_top)).min(2) {
                emit_break(lines, line_count, &mut cur, html_truncated, false, default_fg);
            }
            let indent = usize::from(list_depth.saturating_mul(2)).saturating_add(usize::from(inline_indent));
            for _ in 0..indent.min(16) {
                emit_char_cap(
                    lines,
                    line_count,
                    &mut cur,
                    html_truncated,
                    b' ',
                    cur_fg,
                    usize::from(active_width),
                );
            }
            emit_char_cap(
                lines,
                line_count,
                &mut cur,
                html_truncated,
                b'-',
                cur_fg,
                usize::from(active_width),
            );
            emit_char_cap(
                lines,
                line_count,
                &mut cur,
                html_truncated,
                b' ',
                cur_fg,
                usize::from(active_width),
            );
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
