Eve browser — capabilities and limits
====================================

Eve is a **small in-kernel HTML/CSS subset renderer** (see `kernel/src/html.rs`). It is **not**
Blink, WebKit, Gecko, or Chromium-class software. **Parity with Safari, Chrome, Firefox, Opera, or
Edge is out of scope** for this project: those engines are multi-million-line platforms with full
CSS, layout, scripting, media, and security teams.

What Eve does today
-------------------
- **HTTP/1.0** and **HTTPS (TLS 1.3)** over VirtIO + QEMU user NAT (`10.0.2.0/24`), DNS at
  **`10.0.2.3`** (see `kernel/src/net.rs`, `utm/NETWORK-BROWSER.md`).
- **HTTPS:** TLS 1.3 with in-tree verified provider/trust anchors path (`eve_tls.rs`, `net.rs`).
- **HTML:** block-ish flow, HTML5-ish sectioning tags, `<b>` / `<i>` / `<code>` / `<mark>`, limited
  colors from `<style>` / inline `style`, `rgb()`/`rgba()`, basic entities. Executable **`<script>`**
  bodies are **skipped** (not run). **Non-JS** `type=` scripts (JSON-LD, `application/json`, …) are
  skipped without the “JS stripped” footer. **`<noscript>`** contents **render**; **`<template>`**
  subtrees are skipped. **`<iframe>` / `<object>`** content is skipped; **`<meta>` / `<link>` /
  `<base>`** tags ignored; dangerous **`href`** schemes on `<a>` are ignored for styling.
- **No browser JavaScript engine** for classic `<script>` (no **React**, no hydration, no Vite
  client bundle). An optional in-house **`eve-script:`** bytecode path exists behind SYS
  **BROWSER SCRIPT VM**—that is **not** ECMAScript.
- **No WebAssembly, no plugins, no downloads, no cookies, no mixed-content engine.**

React, “CSS3”, and TempleOS Web Shrine
--------------------------------------
- **React (and Vue, Svelte, Solid, etc.)** almost always need a **JavaScript runtime** in the
  browser. Eve **does not** ship one. A production **`npm run build`** SPA that expects
  `main-*.js` to execute will **not** become interactive in Eve; at best you see whatever **HTML
  shell** the server sent before scripts ran (often a blank or minimal root `div`).
- **“CSS3”** usually means **flexbox, grid, animations, transforms, variables, complex selectors**,
  etc. Eve only implements a **tiny** declarative subset (mostly **color**, **display** hints,
  margins/padding, **width** in `ch`/`px`/`%`). **Layout parity with a real stylesheet is not
  possible** inside this renderer without replacing it with a full engine.
- **TempleOS Web Shrine** (`https://alexanderdfox.github.io/TempleOSWebShrine/`) is the default
  home because it is **small, static-friendly HTML** over HTTPS. If the Shrine site (or your fork)
  is ever rebuilt as a **React SPA**, you must also ship a **static or SSR HTML** path for Eve (or
  accept that Eve users only see the pre-JS document).
- **Practical options for authors**
  - **Static export:** Next.js `output: 'export'`, Vite + prerender, or a plain `index.html` site.
  - **SSR:** Server sends **full HTML** with text content; avoid requiring client JS for the first
    readable paint in Eve.
  - **Dual build:** Same repo builds **SPA for normal browsers** and **`dist-eve/`** static HTML
    for documentation or “kiosk” viewing inside Eve.

Improving rendering further is incremental (fonts, more CSS properties, tables, etc.); reaching
major-browser quality would require a **different architecture** (e.g. a separate user-space
browser process and a much larger engine).
