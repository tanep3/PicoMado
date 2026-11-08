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
