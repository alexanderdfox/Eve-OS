ZealOS / TempleOS lineage and Eve networking
============================================

Zeal Operating System (TempleOS-family, HolyC) lives at:
  https://github.com/Zeal-Operating-System/ZealOS
and is placed in the public domain under the Unlicense.

Eve does **not** embed ZealOS or HolyC sources. The x86_64 guest stack is Rust in
`kernel/src/net.rs` (MIT OR Apache-2.0 per this repo).

The link is **intent**, not a code import: keep a **small, predictable** IPv4 path
(DHCP when available, SLIRP fallback for QEMU user NAT `10.0.2.0/24`, static when set
in SYS) so the in-guest browser can load pages without a full desktop OS network daemon.

For QEMU / UTM specifics see `utm/NETWORK-BROWSER.md` and `utm/NETWORK-QEMU-UTM.md`.
