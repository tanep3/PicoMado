> 公開用注記: 本文のホームパス・ネットワーク値は匿名化した例示値です。時刻付き観測の原値や現在の設定先を示しません。

# Pico → Pi5 Tailscale IPv4 experiment

This is a separate run from the `192.0.2.120` home-LAN destination recorded in `results-2026-10-11-tailscale-on.md`.

- Requested Pi5 destination: `203.0.113.9` (Pi5 Tailscale IPv4).
- Only the `PI_TAILSCALE_IP` definition in Git-ignored `test_config.h` was changed; Wi-Fi settings were preserved without being displayed.
- A diagnostic UF2 was clean-built into the Git-ignored `build/` directory; the ELF contained the requested destination. A single BOOTSEL RP2040 Pico W was identified and this UF2 was transferred, verified, and executed after the receiver and USB monitor were running.
- Pi5's temporary A/C receiver was started on ports 18180/18182 and verified listening **before** Pico flashing.
- User-stated phone conditions: SIM/mobile data, phone Wi-Fi OFF, Tailscale ON. These phone settings were not independently read from the phone.

## Observed result, approximately 04:36–04:38 UTC

- USB log confirmed a fresh `diag-20261011-1` boot with configured Pi5 IPv4 **`203.0.113.9`**. Wi-Fi/IP setup succeeded: `link=UP(3)`, Pico tethering DHCP IPv4 `198.51.100.11/24`, gateway `198.51.100.232`.
- **A (Pico → Pi5 HTTP POST):** Pico attempted TCP to `203.0.113.9:18180` three times. Every attempt ended **during TCP connect** with lwIP `ERR_ABRT` (`-13`). Pi5's already-listening receiver logged no connection. No HTTP POST or `ACK A trial-001` was exchanged.
- **C (Pico-originated persistent TCP):** Pico attempted TCP to `203.0.113.9:18182` three times. Every attempt ended **during TCP connect** with `ERR_ABRT` (`-13`). Pi5 logged no connection. No PUSH or `ACK C trial-001` occurred.
- **B:** not repeated for this destination-change run. The earlier Pi5 → Pico attempt using the observed `198.51.100.11` tethering DHCP address timed out; B has no proven reverse route here.
- The Pi5 temporary receiver and USB monitor were stopped after the bounded attempts. Ports 18180/18182 were free; the Pico remained running its diagnostic firmware.

This directly tested the Pi5 **Tailscale IPv4** as the Pico destination, unlike the prior home-LAN-address run. Under these observed conditions, the tethered Pico did not establish TCP to Pi5's Tailscale IP. The experiment does not isolate whether phone tethering/VPN forwarding, routing, filtering, or another component caused the failure. Phone-to-Pi5 reachability does not prove tethered-Pico-to-Pi5 reachability. No Wi-Fi password or Tailscale key was read or displayed, and no network settings were changed.
