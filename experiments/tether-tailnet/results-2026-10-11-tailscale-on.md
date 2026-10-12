# Tethering reachability run — phone Tailscale ON condition

## Conditions before Pico restart

- User confirmed: phone is using its SIM/mobile connection, Wi-Fi is OFF, and Tailscale is ON.
- User confirmed: the phone itself can reach Pi5. Reachability from a tethered Pico is a separate test.
- Pico diagnostic firmware remains `diag-20261011-1`, configured for Pi5 home LAN IPv4 `192.168.0.120` and test ID `trial-001`.
- Pi5's temporary A/C receiver was started on ports 18180/18182, bound to all local IPv4 addresses, before the Pico's normal power reconnect. The ports were free immediately beforehand.
- The user normally reconnected the Pico's USB power around 2026-10-11 04:22:36 UTC, without BOOTSEL. USB serial showed `probe build=diag-20261011-1 boot` followed by `Wi-Fi/IP connection failed code=-8 link=FAIL`.
- SDK meaning: `-8` is `PICO_ERROR_CONNECT_FAILED`; `CYW43_LINK_FAIL` means the Wi-Fi connection failed. This is not proof of the underlying cause. The firmware returned before obtaining a new DHCP address or starting A/C.
- **A: not attempted in this boot. C: not attempted in this boot. B: not attempted**, because there was no confirmed current Pico DHCP IPv4. The previous `10.101.65.11` belongs to the earlier boot and was not reused as a B target. Pi5's receiver logged no A/C connection.
- The temporary Pi5 receiver and USB monitor were stopped, and ports 18180/18182 were released. This run is separate from the earlier Tailscale OFF result in `results-2026-10-11.md`.

This run confirms only that Wi-Fi/IP setup failed on the Pico under the stated phone conditions. It does **not** establish whether a tethered Pico can reach Pi5 over the phone's Tailscale connection after Wi-Fi succeeds.

## Later normal reconnect reported around 04:25:53 UTC

- The Pi5 receiver had been stopped after the earlier Wi-Fi failure and was restarted **after** this Pico reconnect. It was listening on 18180/18182 when checked. Its log showed no A/C arrival.
- The next observed Pico periodic log reported `diag-20261011-1`, uptime about 77 seconds, `link=UP(3)`, DHCP IPv4 `10.101.65.11/24`, gateway `10.101.65.232`, and Pi5 destination `192.168.0.120`.
- A/C: **indeterminate for this reconnect**. Firmware makes at most three attempts, and the receiver started too late to establish that it was listening during those attempts. No A/C success or ACK was observed; this absence is not evidence that the phone's Tailscale ON path is blocked.
- B: one Pi5 TCP attempt to the newly observed `10.101.65.11:18181` timed out after five seconds; no PUSH/ACK. Pi5's route to that address was via home router `192.168.0.1` on `wlan0`.
- At that point, the Pi5 A/C receiver was left ready for a normal Pico power reconnect **without BOOTSEL**. No firmware, secret setting, or network configuration change was made for that repeat.

## Normal reconnect reported around 04:29:50 UTC — receiver already listening

- Pi5's A/C receiver and Pico USB log monitor were started **before** this Pico reconnect. The receiver bound `0.0.0.0:18180` and `0.0.0.0:18182` with test ID `trial-001` and remained listening through all three Pico attempts.
- USB log confirmed a fresh `diag-20261011-1` boot, Wi-Fi/IP assigned, `link=UP(3)`, Pico DHCP IPv4 `10.101.65.11/24`, gateway `10.101.65.232`, and configured Pi5 **home LAN** IPv4 `192.168.0.120`.
- **A:** Pico attempted TCP to `192.168.0.120:18180` three times. Each attempt ended **during TCP connect** with lwIP `ERR_ABRT` (`-13`). Pi5 receiver logged no arrival; therefore no HTTP POST and no `ACK A trial-001` were exchanged.
- **C:** Pico attempted TCP to `192.168.0.120:18182` three times. Each attempt ended **during TCP connect** with `ERR_ABRT` (`-13`). Pi5 receiver logged no arrival; therefore no persistent connection, PUSH, or `ACK C trial-001` occurred.
- **B:** not repeated on this reconnect. A previous one-shot test under the phone's Tailscale ON condition timed out while connecting from Pi5 to the observed Pico DHCP IPv4 `10.101.65.11:18181`; no B ACK was observed.
- Pi5's temporary receiver was stopped after the run, the USB monitor ended, and ports 18180/18182 were free around 04:31 UTC.

This establishes that **this run did not provide Pico-to-Pi5 TCP reachability at the configured home LAN destination**, even while the phone's Tailscale was reported ON and the Pico had a tethering DHCP lease. It does not identify which route, tethering/VPN forwarding behavior, firewall, or other component blocked the path. It does not test the Pi5 Tailscale IPv4 as the Pico destination, and does not prove that all phone/Tailscale arrangements fail.
