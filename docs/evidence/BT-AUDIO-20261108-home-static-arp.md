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
