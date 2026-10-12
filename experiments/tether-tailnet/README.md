# Pico W ⇄ Pi5 tethering/TailSocks reachability experiment

This is an isolated network probe for the **new** Pico W. It does not use the LCD, buttons, YouTube player, or E-ink device. The three paths each carry a small test ID and ACK:

| Path | Initiator | Proof |
| --- | --- | --- |
| A | Pico → TailSocks SOCKS5 → Pi5 | HTTP POST `TEST A <id>`; Pi5 returns `ACK A <id>` |
| B | Pi5 → Pico tethering DHCP IPv4 | New TCP connection sends `PUSH B <id>`; Pico returns `ACK B <id>` |
| C | Pico → TailSocks SOCKS5 → Pi5, connection kept open | Pi5 sends `PUSH C <id>` on that connection; Pico returns `ACK C <id>` |

With `PROBE_USE_SOCKS5=1` (the default), A/C first connect to TailSocks at `10.101.65.232:48115`, negotiate SOCKS5 username/password authentication, then request a tunnel to Pi5 `100.64.0.9` on the test port. The Pi5 program sees only the application traffic after CONNECT. The Pico prints SOCKS stage and status codes, never credentials. Direct mode remains available with `PROBE_USE_SOCKS5=0`. B is a separate new connection from Pi5 to the Pico's tethering DHCP address; it requires a reverse route and hotspot permission. If B fails but C succeeds, application data can still travel both ways on the Pico-originated C connection. Do not alter routes, VPN, router, or ACL to force a result; record the existing behavior.

## Prepare locally

1. Confirm the only Pico selected for flashing is the **new** test Pico. Check USB identity immediately before any UF2 copy. Do not reset or flash if multiple Picos or an ambiguous drive appear.
2. On the phone, enable the intended 2.4 GHz hotspot and TailSocks. The user reported a TailSocks node `redmi-tailsocks` (`100.64.0.18`) and hotspot-side SOCKS5 address `10.101.65.232:48115`. Check the actual current phone and hotspot state before testing; the firmware does not configure them.
3. Copy `test_config.h.sample` to `test_config.h` if no local file exists. Fill Wi-Fi SSID/password and **`SOCKS5_USERNAME` / `SOCKS5_PASSWORD` locally**; never put them in chat or Git. The existing local file already has blank credential fields for the user to fill. Set `PI_TAILSCALE_IP` to the final Pi5 IPv4 `100.64.0.9`. `PROBE_USE_SOCKS5` defaults to 1; define it as 0 only for a direct-mode comparison. `SOCKS5_PROXY_IP` and `SOCKS5_PROXY_PORT` default to the phone hotspot-side values above. `TEST_ID` must match `--id` below. All network destinations are IPv4 literals; DNS is unused.
4. Build without changing installed dependencies:

   ```sh
   cmake -S experiments/tether-tailnet -B /tmp/picomado-tether-build -DPICO_BOARD=pico_w -DPICO_SDK_PATH=/home/tane/dev/PicoW/pico-sdk -DPICO_NO_PICOTOOL=1
   cmake --build /tmp/picomado-tether-build -j2
   picotool uf2 convert /tmp/picomado-tether-build/tether_tailnet_probe.elf /tmp/picomado-tether-build/tether_tailnet_probe.uf2
   ```

   The installed picotool is 2.1.1 while this SDK requires 2.3.0 for its automatic UF2 step. This build disables that step and uses the installed tool's ELF-to-UF2 conversion for Pico W (RP2040). The UF2 is `/tmp/picomado-tether-build/tether_tailnet_probe.uf2`. It contains the locally entered Wi-Fi and SOCKS5 credentials, so keep it out of Git and do not upload it. It is **not** flashed automatically. The project's `experiments/tether-tailnet/build/` is also Git-ignored for local builds.

   Before flashing, the protocol framing can be checked without any local credentials or hardware:

   ```sh
   cc -std=c11 -Wall -Wextra -Werror experiments/tether-tailnet/test_socks5_wire.c -o /tmp/picomado-socks-wire-test
   /tmp/picomado-socks-wire-test
   ```

## Run after target confirmation

On Pi5, check A/C ports are still unused and start the short-lived receiver **before** starting the Pico. Bind to the Pi5 Tailscale IPv4 (not the phone proxy or Pico DHCP IPv4):

```sh
ss -ltn '( sport = :18180 or sport = :18182 )'
python3 experiments/tether-tailnet/pi_probe.py serve --bind 100.64.0.9 --id trial-001
```

Flash the UF2 only to the uniquely identified new Pico. Watch its USB serial log. The new build prints `socks5-20261011-1` at boot and every 10 seconds, so it can be identified even if USB serial was opened after startup. Status includes Wi-Fi/IP link state, Pico DHCP IPv4, netmask, gateway, final Pi5 IPv4, and proxy mode; it never prints SSID or passwords. A and C start automatically, make at most three attempts each, and log TCP, SOCKS method, authentication status, CONNECT status, application ACK, or bounded timeout by stage. Their SOCKS TCP and CONNECT replies can arrive in pieces. The Pi5 process prints each received ID and its ACK; C additionally checks the Pico's ACK to prove Pi5→Pico PUSH and Pico→Pi5 response on one outbound connection.

For B, use the **Pico DHCP IPv4 printed by the Pico**, not either Pi5 address (`192.168.0.120` or `100.64.0.9`). On Pi5, inspect the current reverse route and then try the new connection:

```sh
ip -4 route get <PICO_DHCP_IPV4>
python3 experiments/tether-tailnet/pi_probe.py push-b --pico <PICO_DHCP_IPV4> --id trial-001
```

The B command reports success only when the matching ACK arrives. A phone may isolate tethered clients or give no reverse route, so B failure is a valid test result. Stop the Pi5 receiver with Ctrl+C. The Pico firmware has no background service on Pi5 and no boot persistence.

Record A/B/C separately with the test ID, Pico DHCP IPv4, chosen Pi5 IPv4, phone model/OS, route observation, and the relevant log lines. Never record SSID password or Tailscale keys.
