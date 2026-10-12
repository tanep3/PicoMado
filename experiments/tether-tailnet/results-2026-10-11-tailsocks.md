# TailSocks SOCKS5 reachability run — 2026-10-11

Run approximately 07:09–07:11 UTC. This is separate from the earlier direct Pico→Pi5 LAN and Tailscale-IP runs.

## Setup and identity

- A single BOOTSEL RP2040 Pico W was identified before flashing, with the same flash ID `E6614864D34F8E22` as the earlier experiment Pico. No other Pico was flashed.
- The locally entered, Git-ignored configuration was built without displaying credentials. The new UF2 was transferred with verification and executed. USB log identified the running build as `socks5-20261011-1`, mode `SOCKS5`.
- Pico hotspot DHCP IPv4: `10.101.65.11/24`; gateway and TailSocks SOCKS5 endpoint: `10.101.65.232:48115`. Final Pi5 destination: Tailscale IPv4 `100.64.0.9`.
- The user reported TailSocks v4.9.1 as the independent Headscale node `redmi-tailsocks` (`100.64.0.18`). Pi5 independently observed inbound A/C connections **from `100.64.0.18` to `100.64.0.9`**.
- Pi5's temporary A/C receiver was listening on `100.64.0.9:18180` and `100.64.0.9:18182` before the Pico UF2 was transferred. USB serial monitoring also started before transfer.

## Stage-by-stage evidence, test ID `trial-001`

| Stage | Pico observation | Pi5 observation | Result |
| --- | --- | --- | --- |
| Wi-Fi/IP | `link=UP(3)`, DHCP `10.101.65.11` | — | Succeeded |
| SOCKS5 proxy TCP | A and C each logged TCP connected to SOCKS5 proxy | — | Succeeded |
| SOCKS5 username/password auth | A and C each logged authentication accepted; credentials were not printed | — | Succeeded |
| SOCKS5 CONNECT | A and C each logged tunnel established to Pi5 `100.64.0.9` | A/C arrived from `100.64.0.18` | Succeeded |
| A: Pico→Pi5 HTTP POST | `A application sent, id=trial-001`, then `A OK: HTTP POST acknowledged` | `A POST received: id=trial-001 from=100.64.0.18 to=100.64.0.9`; `A ACK sent` | **POST and matching ACK succeeded** |
| C: Pico-started persistent TCP | `C application sent`, then `C PUSH received and ACK sent, id=trial-001` | `C persistent connection accepted: id=trial-001 from=100.64.0.18 to=100.64.0.9`; `C ACK OK: 'ACK C trial-001'` | **Pi5→Pico PUSH and Pico→Pi5 ACK succeeded on the same Pico-originated connection** |

B (a separate, new Pi5→Pico connection) was not repeated in this SOCKS5 run. Its previous one-shot direct attempt timed out. **C proves application-level data flow in both directions without requiring B.** The short test does not establish long-duration connection stability, throughput, or behavior after phone network changes.

The temporary Pi5 receiver and USB monitor were stopped after the successful run; ports 18180/18182 were released. No OS, router, VPN, ACL, or existing production service setting was changed. No credentials or keys were logged, committed, or uploaded.
