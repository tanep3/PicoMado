# E-ink_1 HOME static-ARP diagnostic, 2026-11-08

Specification: `d17e895`, existing BT-AUDIO-20261107 problem record. This is a
bounded mapping comparison, not an accepted repair or hardware application.

## Implementation and offline checks

`PM_HOME_STATIC_ARP_DIAGNOSTIC` defaults OFF and CMake rejects ON for another
device. Main passes the explicit current profile identity to the diagnostic
network entry point. HOME2/AWAY emit a skip marker and do not pin, including
HOME2 with the identical server address. HOME requires E-ink_1, direct server
192.168.0.120 and an UP/link-UP, assigned, same-subnet IPv4 route. The network
owner removes any previous owned entry, adds 2c:cf:67:11:72:f3 and checks readback
before creating the new TCP PCB, all inside its existing CYW43 lock. Fixed and
failure markers identify the connection; existing snapshots show the MAC.

The installed SDK uses **ETHARP_SUPPORT_STATIC_ENTRIES**, not
LWIP_ARP_STATIC_ENTRIES. In `lib/lwip/src/core/ipv4/etharp.c`, add selects the
route and calls the actual table updater (504–519); static entries reject
ordinary conflicting updates (445–453), are excluded from aging (208), and
netif cleanup removes them (560 onward). Removal returns ERR_OK for deletion,
ERR_MEM when absent, and ERR_ARG for a dynamic entry, which it leaves alone
(530–551). Only those three outcomes release local ownership. An unexpected
removal error retains ownership and prevents the next connection, including
HOME2, rather than leaking a static mapping into another profile.

Serial host checks: `firmware/tests/run_home_static_arp_tests.py --sdk
/home/tane/dev/PicoW/pico-sdk --out /tmp/picomado-static-arp-host-20261108`.
All 27 compile/run/configure steps passed, with native and ASan+UBSan variants.
LeakSanitizer is disabled under the executor. Fixtures use only deterministic
memory/network adapters, not real sockets, radio, USB or SDK mutations.

- Production net: HOME scope, other identity, HOME2/AWAY/unknown skip,
  pre-TCP ordering and lock assertions, no-route/down/link-down/no-address/
  off-subnet/no-mask rejection, add/readback failure, retry/removal,
  allocation/connect failures and SDK-cleaned/dynamic/unknown removal outcomes.
- Installed ARP source: dynamic replacement, conflicting update refusal, static
  aging exclusion, explicit removal, dynamic-entry preservation, netif cleanup
  and missing route. This executes the installed implementation, not a model.
- Production main dispatch: diagnostic HOME/HOME2/AWAY identity forwarding;
  ordinary HOME/HOME2 path. The pre-existing app fixture routes ordinary AWAY
  through its separate EEX1 tests; this focused ordinary dispatch does not claim
  to execute that path. BT is compiled out in these dispatch fixtures.
- Existing ordinary network and terminal cleanup fixtures pass unchanged.
- Default OFF and E-ink_1 ON CMake accepted; other-device ON rejected.

Private JSON results and source hashes are in the above output directory.
Full aggregate/Cargo tests and independent acceptance are not claimed. Regular
ARM builds, exact effective compile definitions, UF2/source/config/SDK hashes,
normal instruction comparison and hardware results are recorded separately.
No USB, SSH, service, network-settings, private-config or playback operation
was performed by this implementation worker.

## Regular ARM comparison artifacts

Implementation commit: `5c9dc87fdc7317a5bcf36680c995086890d59fbf`.
Both separate regular CMake builds completed serially with `-j1`; no manual
object substitution was used. Private staging is
`/tmp/picomado-static-arp-deploy.raeq4x2d/` (directory0700, artifacts0600).

- Normal: `normal-E-ink_1.uf2`, 1,095,680 bytes, SHA256
  `ecaad23d53a85ef64b042138ed27c2f7655402c231efaf5d8ff8f17cc4d8516a`.
  This is **byte-identical to the preserved original normal UF2**, not merely
  a source-level or instruction-level comparison.
- Fixed HOME: `fixed-E-ink_1.uf2`, 1,097,728 bytes, SHA256
  `d8d78feccb40965e8188e4f21e13921abee43d33a6abaa22cf065bd86a0be204`.
  ELF text554768/data0/BSS166616. The original and normal ELF are
  text553768/data0/BSS166608.
- Final `manifest.json` SHA256
  `889140927cc1192cbcc3d1157dd0ffd324d51774479d4ebf3cc1bdf19445f0d8`.
  It binds production source hashes, config hash, SDK commit/submodules,
  compilation/build commands and logs, ELF/UF2 hashes, flag comparison,
  structural UF2 validation and original-artifact invariants. No private
  configuration values were printed or added to this repository.

The only effective definition differences in the regular pair are
`PM_HOME_STATIC_ARP_DIAGNOSTIC=1` and `ETHARP_SUPPORT_STATIC_ENTRIES=1`.
Includes and compiler flags are equal after build-directory normalization.
`fixed-selected-compile.json` and `normal-selected-compile.json` retain the
actual main.c/net.c/**SDK etharp.c** compilation commands and verify both
definitions reach SDK compilation only in the fixed build. SDK source and
submodules, private device config, original build cache/flags/UF2 are unchanged.
Both UF2s pass magic/family/block/index/address checks and do not overlap the
last reserved12KiB. Normal main/net instructions and relocations also match
the original objects after stripping only objdump's filename header.

The existing `run_bluetooth_startup_tests.py` regression also passes, including
modes0/1/2, BT-OFF, production main-loop and C/CMake boundaries. Its additional
private log is `/tmp/picomado-static-arp-bluetooth-regression.log`. The fixed
marker fits128bytes even at maximum timestamp/connection values, below the
existing144-byte trace limit. `git diff --check` passes.

Parent handoff: compare the already captured ordinary HOME start with a fresh
fixed start under the same stopped-audio conditions. Require `event=fixed`
before the connection's TCP traffic, read-back MAC2c:cf:67:11:72:f3 and retained
trace quality. HOME2/AWAY should emit `event=skip`; mapping/route failures are
failed diagnostic connections, not successful fixed trials. Track ACK-position
progress and host Receipt/Rendered separately. Restore the preserved normal
UF2 afterward; neither image writes a persistent static mapping. Device
application and causal effectiveness require the parent's actual evidence.

## Authorized E-ink_1 HOME A/B/A hardware comparison

On 2026-11-08 the integration worker ran normal → fixed → restored-normal starts on USB identity E6614864D34F8E22. Each observation lasted 150 seconds with local-music selected but paused. No audio playback, router change, other-device change or SSH was initiated. Current volume 38 was preserved. The ARM builds finished before the fixed and restoration trials; no build was running in these observation windows. The server was the same streaming deployment throughout.

- Normal before, capture `153352`: initial connection 1 used router MAC10:3c:59:36:fb:2d for server192.168.0.120. Its ACK stayed7177 while the send end advanced7383→7755; tcp_error-13 followed. Connection3 recovered with real server MAC2c:cf:67:11:72:f3 and ACK caught up. The first new host Rendered sample appeared90.03seconds after capture start (frame2440243).
- Fixed, capture `154529`: `event=fixed result=0` occurred at MCU11.131seconds on HOME, before TCP traffic. All9 valid TCP snapshots remained connection1, with ACK equal to send end and unsent/unacked both0. No TCP error or reconnection occurred. Device render completed at MCU17.946720seconds; the host's first new Rendered sample was20.018seconds after capture start (frame2440244). All observed peer MACs were the real server MAC.
- Normal restored, capture `154824`: verified the original ecaad23... UF2 on the same device. No static marker remained. Router MAC and ACK7177 stall returned, with send end7659→7757, then tcp_error-13. Connection3 recovered with the real server MAC. The first new host Rendered sample appeared90.049seconds after capture start (frame2440246). The device had locally rendered the earlier frame2440245 at17.866280seconds, but that did not establish delivery of its completion to the host.

All three logs report dropped=0. USB arrival byte counts are retained with complete logs and state samples. Old host Rendered values observed immediately after reboot were excluded. The 5-second state sampling limits host-time precision; device rendering and host acknowledgment are separate measurements.

The reversible stall→clear→stall result strongly supports the ARP-selected path through the router as a contributor to this startup failure. It does not identify the router's internal dropping mechanism, prove that every router-MAC path fails, or establish performance under every environment. Earlier successful router-MAC transfers remain valid counterexamples to a blanket claim. No permanent network fix or broad static-MAC rollout is claimed.

The final device is the original normal firmware, online and Bluetooth connected after recovery. USB was closed. The temporary static entry existed only in RAM and was removed by the normal-firmware reboot. Private artifacts, manifests and all three captures are retained under `/home/tane/picomado-staging/20261108-eink1-home-static-arp/`. Independent approval and final issue closure remain parent-owned.

## External DHCP: read-only reconciliation

The user subsequently stated that DHCP on L13 is disabled and dnsmasq on an
LXD system at 192.168.0.160 manages IP allocation. On 2026-11-08 the investigator
read only raspi5's already acquired state; no request was sent to that LXD
system, no remote login/SSH occurred, and no DHCP release/renew, packet capture,
router login, network setting or device/service change was performed.

- NetworkManager's current eth0 DHCP options identify server 192.168.0.160,
  address 192.168.0.120, subnet mask 255.255.255.0, router 192.168.0.1 and an
  86400-second lease. Current LAN routing uses eth0's connected 192.168.0.0/24 route.
- wlan0 is unmanaged by NetworkManager. Its existing systemd-networkd lease,
  read from `/run/systemd/netif/leases/<wlan0-ifindex>`, identifies the same
  DHCP server 192.168.0.160, address 192.168.0.121, mask 255.255.255.0,
  router 192.168.0.1 and a one-day lifetime. Both management services are active;
  that fact alone does not establish conflicting management of either NIC.
- The retained host options/lease and current routes show no classless route
  that sends Pico LAN traffic through the gateway. This is not a complete
  capture of every offered DHCP option, nor proof that no other DHCP server
  exists. A selected lease identifies the selected server only.

Pico's previously captured `peer=192.168.0.120 hop=192.168.0.120` has a narrower
but useful meaning: `firmware/src/net.c:pm_net_tcp_snapshot` compares the peer
with the live netif address/mask and substitutes the gateway only for an
off-subnet target. Thus those samples identify .120 as on-link, rather than
showing a mistaken subnet mask making Pico ARP for gateway .1. They do not
retain the exact Pico netmask, selected DHCP server or complete offered options.

The installed SDK's `lib/lwip/src/core/ipv4/dhcp.c` requests/parses the ordinary
subnet-mask and router options and copies the first offered router. Its parser
has no option 121/249 classless-route case; unknown options reach the default
`LWIP_HOOK_DHCP_PARSE_OPTION`, which is a no-op unless overridden. No such
override or IP-route hook was found in project C/CMake sources or the known
normal E-ink_1 build's effective definition list. This does not support a
classless DHCP route having directed this Pico to .1. Host clients can support
option 121 independently; see [RFC 3442](https://www.rfc-editor.org/rfc/rfc3442).

External DHCP is a normal separation of address allocation from the gateway;
it does not by itself explain L13 answering .120's ARP request with its own
MAC. The earlier actual competing replies and reversible static-ARP result
remain the stronger path evidence. A L13 learning/proxy interaction with
externally allocated clients remains possible but unverified; no inspected
official documentation establishes such a dependency. The dnsmasq manual's
[PXE proxy mode](https://thekelleys.org.uk/dnsmasq/docs/dnsmasq-man.html) is not
the same as IP-layer proxy ARP.

Still unverified: dnsmasq's actual configuration/host reservations and option
tags, duplicate allocations or competing DHCP offers, Pico's complete acquired
DHCP state, and L13's neighbor/proxy/forwarding state. No causal finding against
external dnsmasq, no configuration correction and no issue closure is claimed.
