#!/usr/bin/env bash
# Build Eve-Pi3.utm / Eve-Pi4.utm bundles (double-click in UTM — no manual QEMU args).
# Prereq: utm/rpi/kernel8-pi*.img (run ./scripts/rpi-utm-sync.sh first).
# Usage: ./scripts/rpi-utm-mkbundle.sh [pi3|pi4|both]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

SCOPE="${1:-both}"

write_launcher() {
  local soc="$1"
  local name script
  case "$soc" in
    pi3) name="Launch-Eve-Pi3" ;;
    pi4) name="Launch-Eve-Pi4" ;;
  esac
  script="$ROOT/utm/rpi/${name}.command"
  cat >"$script" <<EOF
#!/bin/bash
# One-click Eve Pi (QEMU directly — avoids UTM 4.7.5 virtio-serial-pci on raspi*).
# Needs: brew install qemu  (or qemu-system-aarch64 on PATH)
cd "$ROOT"
exec ./scripts/run-raspi-qemu.sh ${soc}
EOF
  chmod +x "$script"
  echo "OK: $script"
}

write_bundle() {
  local soc="$1"          # pi3 | pi4
  local target mem cpus name kernel vm_uuid drive_uuid bundle out_dir data_dir

  case "$soc" in
    pi3)
      target=raspi3b
      mem=1024
      cpus=4
      name="Eve-Pi3"
      kernel="kernel8-pi3.img"
      ;;
    pi4)
      target=raspi4b
      mem=2048
      cpus=4
      name="Eve-Pi4"
      kernel="kernel8-pi4.img"
      ;;
    *)
      echo "usage: $0 [pi3|pi4|both]" >&2
      exit 1
      ;;
  esac

  if [[ ! -f "$ROOT/utm/rpi/$kernel" ]]; then
    echo "error: missing $ROOT/utm/rpi/$kernel — run ./scripts/rpi-utm-sync.sh" >&2
    exit 1
  fi

  vm_uuid="$(uuidgen | tr '[:lower:]' '[:upper:]')"
  drive_uuid="$(uuidgen | tr '[:lower:]' '[:upper:]')"
  bundle="$ROOT/utm/rpi/${name}.utm"
  out_dir="$bundle"
  data_dir="$out_dir/Data"

  rm -rf "$out_dir"
  mkdir -p "$data_dir"
  cp -f "$ROOT/utm/rpi/$kernel" "$data_dir/$kernel"

  # CPUCount must be set explicitly: 0 = host logical CPUs, but raspi3b/raspi4b cap at 4.
  # Network: usb-net via extra args (see utm/qemu-extra-rpi.args). No virtio-net (breaks raspi).
  # Display: empty → UTM adds -nographic; -display default overrides so the Pi framebuffer window works.
  # Serial: built-in UTM terminal → PL011 keyboard (see utm/RPI-UTM-SETUP.md).
  read -r -d '' PLIST <<EOF || true
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Backend</key>
	<string>QEMU</string>
	<key>ConfigurationVersion</key>
	<integer>4</integer>
	<key>Display</key>
	<array/>
	<key>Drive</key>
	<array>
		<dict>
			<key>Identifier</key>
			<string>${drive_uuid}</string>
			<key>ImageName</key>
			<string>${kernel}</string>
			<key>ImageType</key>
			<string>LinuxKernel</string>
			<key>Interface</key>
			<string>None</string>
			<key>InterfaceVersion</key>
			<integer>1</integer>
			<key>ReadOnly</key>
			<true/>
		</dict>
	</array>
	<key>Information</key>
	<dict>
		<key>Icon</key>
		<string>linux</string>
		<key>IconCustom</key>
		<false/>
		<key>Name</key>
		<string>${name}</string>
		<key>Notes</key>
		<string>Eve OS — Raspberry Pi (${target}). UTM 4.7.5: use Launch-${name}.command (UTM injects virtio-serial-pci; no PCI on raspi). Future UTM with Pi support: open this .utm. Keyboard: serial terminal. Rebuild: make utm-rpi</string>
		<key>UUID</key>
		<string>${vm_uuid}</string>
	</dict>
	<key>Input</key>
	<dict>
		<key>MaximumUsbShare</key>
		<integer>0</integer>
		<key>UsbBusSupport</key>
		<string>Disabled</string>
		<key>UsbSharing</key>
		<false/>
	</dict>
	<key>Network</key>
	<array/>
	<key>QEMU</key>
	<dict>
		<key>AdditionalArguments</key>
		<array>
			<string>-monitor</string>
			<string>none</string>
			<string>-display</string>
			<string>default</string>
			<string>-usb</string>
			<string>-netdev</string>
			<string>user,id=rpi0,ipv6=off</string>
			<string>-device</string>
			<string>usb-net,netdev=rpi0</string>
		</array>
		<key>BalloonDevice</key>
		<false/>
		<key>DebugLog</key>
		<false/>
		<key>Hypervisor</key>
		<false/>
		<key>PS2Controller</key>
		<false/>
		<key>RNGDevice</key>
		<false/>
		<key>RTCLocalTime</key>
		<false/>
		<key>TPMDevice</key>
		<false/>
		<key>TSO</key>
		<false/>
		<key>UEFIBoot</key>
		<false/>
	</dict>
	<key>Serial</key>
	<array>
		<dict>
			<key>Mode</key>
			<string>Terminal</string>
			<key>Target</key>
			<string>Auto</string>
			<key>Terminal</key>
			<dict>
				<key>Font</key>
				<string>Menlo</string>
				<key>FontSize</key>
				<integer>12</integer>
				<key>Theme</key>
				<string>Default</string>
			</dict>
		</dict>
	</array>
	<key>Sharing</key>
	<dict>
		<key>ClipboardSharing</key>
		<false/>
		<key>DirectoryShareMode</key>
		<string>None</string>
		<key>DirectoryShareReadOnly</key>
		<false/>
	</dict>
	<key>Sound</key>
	<array/>
	<key>System</key>
	<dict>
		<key>Architecture</key>
		<string>aarch64</string>
		<key>CPU</key>
		<string>default</string>
		<key>CPUCount</key>
		<integer>${cpus}</integer>
		<key>CPUFlagsAdd</key>
		<array/>
		<key>CPUFlagsRemove</key>
		<array/>
		<key>ForceMulticore</key>
		<false/>
		<key>JITCacheSize</key>
		<integer>0</integer>
		<key>MemorySize</key>
		<integer>${mem}</integer>
		<key>Target</key>
		<string>${target}</string>
	</dict>
</dict>
</plist>
EOF

  printf '%s' "$PLIST" >"$out_dir/config.plist"
  plutil -lint "$out_dir/config.plist" >/dev/null

  echo "OK: $bundle"
  ls -la "$data_dir/$kernel" "$out_dir/config.plist"
}

case "$SCOPE" in
  pi3)
    write_bundle pi3
    write_launcher pi3
    ;;
  pi4)
    write_bundle pi4
    write_launcher pi4
    ;;
  both)
    write_bundle pi3
    write_bundle pi4
    write_launcher pi3
    write_launcher pi4
    ;;
  *)
    echo "usage: $0 [pi3|pi4|both]" >&2
    exit 1
    ;;
esac

echo ""
echo "UTM 4.7.5: double-click utm/rpi/Launch-Eve-Pi3.command (or Pi4) — not the .utm bundle."
echo "  (.utm needs a UTM build with experimental Pi support; 4.7.5 adds virtio-serial-pci and fails.)"
echo "After kernel rebuild: make utm-rpi"
