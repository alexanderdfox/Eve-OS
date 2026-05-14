Eve — networking for UTM / QEMU (browser)
=========================================

Goal: the in-guest **VirtIO** (or **e1000**) NIC + **QEMU user NAT (SLIRP)** so Eve can **resolve DNS** and **fetch a page**
(HTTP or HTTPS). Traffic **passes through** QEMU to the **host’s** network (Wi‑Fi/Ethernet); that is not PCI passthrough.

**UTM “Bridged” vs “Shared”:** Eve’s stack is fixed to **10.0.2.x** — use **Shared / NAT** (same as `-netdev user`), not **Bridged**, unless you add DHCP to the kernel. Full detail: **`utm/NETWORK-QEMU-UTM.md`**.

QEMU (repo `cargo run` / `eve-os`)
----------------------------------
The project’s extra arguments add **virtio-net** and **`-netdev user`** with an explicit **`net=10.0.2.0/24,host=10.0.2.2`** so the guest matches `kernel/src/net.rs`. See:

  utm/qemu-extra.args
  utm/qemu-extra-q35.args

Override the netdev string with env **`EVE_QEMU_NETDEV`** (see **`utm/NETWORK-QEMU-UTM.md`**).

Kernel expectations (`kernel/src/net.rs`):

- Guest address: **`10.0.2.15`**
- Gateway: **`10.0.2.2`**
- DNS: **`10.0.2.3`** (QEMU’s built-in resolver to the host)

In Eve **Settings (SYS)**: leave **NIC** on **VirtIO** (or **E1000** if the VM uses `-device e1000`), turn **INTERNET** (stack) **on**,
and set **IP MODE** to **SLIRP** for QEMU user-NAT (that is the default in fresh settings).

**Default home URL** (in `kernel/src/gfx.rs`): **`https://alexanderdfox.github.io/TempleOSWebShrine/`** — loads over the Internet when DNS + TLS succeed (no host server required).

**Local QEMU demo page** on the **host**: from the Eve repo run:

  python3 -m http.server 8080 --directory demo/qemu-http-test

Then in Eve’s URL bar open **`http://10.0.2.2:8080/`** (QEMU maps **`10.0.2.2`** to the host). Press **Enter** in the URL bar or **GO** / **R** to reload. Press **F6** if the URL bar is hidden (BIOS-style full-page browser).

Other URLs: **`https://example.com/`** works if DNS + TLS succeed (see **`utm/BROWSER-LIMITS.md`** for HTTPS limits).
Literal IPs avoid DNS, e.g. **`http://10.0.2.2:8080/`**.

UTM (macOS)
-----------
1. Attach **`utm/qemu-extra.args`** (PC) or **`utm/qemu-extra-q35.args`** (Q35/UEFI) in UTM’s
   **Additional QEMU arguments** field — see **`utm/UTM-SETUP.md`** §4.
2. Ensure a **virtio-net-pci** (or **e1000**) device is present (those files use **virtio-net-pci**).
3. Same SYS toggles as above.

Troubleshooting — “web pages do not load”
-----------------------------------------
1. **Default home:** **`https://alexanderdfox.github.io/TempleOSWebShrine/`** — needs working **DNS + TLS** (see **`utm/BROWSER-LIMITS.md`** if verification fails).

2. **Host-only demo:** To try **`http://10.0.2.2:8080/`** instead, on the **host** run  
   `python3 -m http.server 8080 --directory demo/qemu-http-test` (see **`demo/qemu-http-test/README.md`**).  
   Without that service on `10.0.2.2:8080`, that specific URL shows **TCP NO CONNECT** or a blank page.

3. **SYS → General:** **INTERNET** must be **on**, **NIC** must **not** be **OFF**, **IP MODE** **SLIRP** for QEMU/UTM **Shared/NAT** (`10.0.2.15` / gateway `10.0.2.2` / DNS `10.0.2.3`).

4. **VirtIO / netdev:** The VM needs **virtio-net** (or e1000 per your image) **and** **`-netdev user`** (or UTM **Shared Network**).  
   **`NET: NODRV` / `nic none`:** add the lines from **`utm/qemu-extra.args`** (x86) or run **`scripts/arm-uefi-run.sh`** (AArch64 QEMU), which includes user NAT + virtio-net.

5. **Apple Silicon / Asahi native UEFI:** There is **no** VirtIO-MMIO NIC on real hardware — the browser **cannot** reach the network until a driver exists. Use **QEMU** for the networked browser demo.

6. **DHCP:** If you set **IP MODE** to **DHCP** and nothing answers, the stack **falls back to SLIRP** after a timeout. On a **bridged** LAN without DHCP, use **STATIC** with correct IP/gateway/DNS.

7. **DNS / HTTPS:** **`DNS TIMEOUT`** → check user netdev / DNS forward. **`TLS VERIFY FAIL`** / **`TLS BAD HOST`** → see **`utm/BROWSER-LIMITS.md`**.

8. **Stuck on “GET” / loading:** Older Eve waited for the TCP connection to **close** before showing the page, so **HTTP/1.1 keep-alive** (no FIN) looked hung forever. The kernel now honors **`Content-Length`** and still times out TCP connect (**`TCP NO CONNECT`**) if the handshake never completes. **`Transfer-Encoding: chunked`** is not decoded yet; those sites may still need the server to close the connection or may hit **`HTTP TIMEOUT`**.

9. **Reload:** After changing SYS network settings, open the browser and press **Enter** in the URL bar or click **GO** / **R**.

See also **`utm/ZEALOS-NET-NOTE.md`** (design note).
