// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! Minimal DOM/style/layout scaffolding for incremental browser-engine upgrades.

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Element,
    Text,
}

#[derive(Clone, Copy)]
pub struct StyleHints {
    pub display_none: bool,
    pub indent_spaces: u8,
}

impl StyleHints {
    pub const fn new() -> Self {
        Self {
            display_none: false,
            indent_spaces: 0,
        }
    }
}

#[derive(Clone, Copy)]
pub struct DomNode {
    pub kind: NodeKind,
    pub parent: Option<u16>,
    pub first_child: Option<u16>,
    pub next_sibling: Option<u16>,
    pub text_start: u16,
    pub text_len: u16,
    pub style: StyleHints,
}

impl DomNode {
    const fn empty() -> Self {
        Self {
            kind: NodeKind::Element,
            parent: None,
            first_child: None,
            next_sibling: None,
            text_start: 0,
            text_len: 0,
            style: StyleHints::new(),
        }
    }
}

pub const DOM_MAX_NODES: usize = 512;
pub const DOM_TEXT_ARENA: usize = 8192;

pub struct DomTree {
    pub nodes: [DomNode; DOM_MAX_NODES],
    pub node_count: usize,
    pub text: [u8; DOM_TEXT_ARENA],
    pub text_len: usize,
}

impl DomTree {
    pub const fn new() -> Self {
        Self {
            nodes: [DomNode::empty(); DOM_MAX_NODES],
            node_count: 0,
            text: [0; DOM_TEXT_ARENA],
            text_len: 0,
        }
    }

    pub fn reset(&mut self) {
        self.node_count = 0;
        self.text_len = 0;
    }
}

/// Parse a tiny text-only DOM skeleton from raw bytes.
///
/// This deliberately avoids allocations and full HTML compliance; it exists so future
/// CSS/layout/scripting work can target a stable tree API instead of line-only text.
pub fn build_text_dom(raw: &[u8], out: &mut DomTree) {
    out.reset();
    // Node 0: synthetic root.
    if out.node_count < out.nodes.len() {
        out.nodes[0] = DomNode::empty();
        out.node_count = 1;
    }
    let mut cur_text_start = out.text_len;
    for &b in raw {
        if b == b'<' || b == b'\r' {
            continue;
        }
        if b == b'\n' {
            if out.text_len > cur_text_start {
                push_text_node(out, cur_text_start, out.text_len - cur_text_start);
            }
            cur_text_start = out.text_len;
            continue;
        }
        if out.text_len < out.text.len() {
            out.text[out.text_len] = b;
            out.text_len += 1;
        }
    }
    if out.text_len > cur_text_start {
        push_text_node(out, cur_text_start, out.text_len - cur_text_start);
    }
}

fn push_text_node(out: &mut DomTree, start: usize, len: usize) {
    if out.node_count >= out.nodes.len() {
        return;
    }
    let idx = out.node_count;
    out.node_count += 1;
    out.nodes[idx] = DomNode {
        kind: NodeKind::Text,
        parent: Some(0),
        first_child: None,
        next_sibling: None,
        text_start: start.min(u16::MAX as usize) as u16,
        text_len: len.min(u16::MAX as usize) as u16,
        style: StyleHints::new(),
    };
    // Append as root child/sibling list.
    if out.nodes[0].first_child.is_none() {
        out.nodes[0].first_child = Some(idx as u16);
        return;
    }
    let mut n = out.nodes[0].first_child.unwrap_or(0) as usize;
    while let Some(nx) = out.nodes[n].next_sibling {
        n = nx as usize;
    }
    out.nodes[n].next_sibling = Some(idx as u16);
}
