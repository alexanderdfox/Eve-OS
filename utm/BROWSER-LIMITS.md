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
- **HTML:** block-ish flow, `<b>` / `<i>`, limited colors from `<style>` / inline `style`, basic
  entities. **`<script>` is stripped** (never executed). **`<iframe>` / `<object>`** content is
  skipped; **`<meta>` / `<link>` / `<base>`** tags ignored; dangerous **`href`** schemes on `<a>` are
  ignored for styling.
- **No browser JavaScript engine for `<script>` tags** (they are stripped), but an optional
  in-house `eve-script:` bytecode marker VM path exists behind SYS toggle (`BROWSER SCRIPT VM`).
- **No WebAssembly, no plugins, no downloads, no cookies, no mixed-content engine.**

Improving rendering further is incremental (fonts, more CSS, tables, etc.); reaching major-browser
quality would require a different architecture (e.g. a separate user-space browser process and a
much larger engine).
