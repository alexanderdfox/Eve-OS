Eve — minimal HTTP page for QEMU user networking
=================================================

Eve’s kernel only speaks plain HTTP (no TLS). In QEMU/UTM with
`-netdev user`, the host is reachable from the guest as **10.0.2.2**.

From the Eve repo root on your Mac/Linux host:

  python3 -m http.server 8080 --directory demo/qemu-http-test

In the Eve browser URL bar (VirtIO NIC + Internet stack ON in SYS):

  http://10.0.2.2:8080/

Stop the server with Ctrl+C when done.

See also: utm/UTM-SETUP.txt (Internet section).
