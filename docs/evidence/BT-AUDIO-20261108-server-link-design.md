# Server link hint design checks — no runtime implementation

Authority: [BT-AUDIO-20261107](../changes/BT-AUDIO-20261107.md); design
[server-link-hint.md](../design/server-link-hint.md), initial specification
commit `a335841`. Host NIC/route investigation is separate. These checks test
the proposal's model and existing behavior; they do not implement a Welcome
MAC hint, prove RF delivery or apply firmware/service/network changes.

## Executed results

`python3 /tmp/picomado-server-link-design/check.py` completed successfully.
45 pure reference-model checks and8 native/ASan+UBSan compile/run steps passed.
LeakSanitizer was disabled because of the executor, not a claimed leak check.

1. **Actual current firmware Welcome handler:** old Welcome and Welcome with
   additive `server_link` object both produce the same SESSION→ACTIVE transition
   and outer incarnation/session. Duplicate Welcome in ACTIVE and Welcome in
   WIFI/SERVER/RETRY are ignored. No MAC support exists in that handler yet.
   This verifies old-firmware additive compatibility and the insertion point;
   it does not test a future field parser or new-server serializer.
2. **Actual installed SDK subsequent output:** use the same netif and packet
   with real `etharp_output` and `etharp_add_static_entry`. The captured mock
   Ethernet destination is router MAC before installation, host MAC after
   installation, still host MAC after a conflicting dynamic update, then router
   MAC again after removing the pin and learning the dynamic mapping. No TCP
   recreation is needed for that ARP-table effect. This is an actual SDK
   Ethernet-adapter test, not a real TCP/RF delivery or timing result.
3. **Proposed pure model:** HOME/HOME2, current connection/join/profile/netif/
   address/prefix identity, direct-route requirement, accepted local/peer IP
   matches, version/capability, missing hint, valid locally administered MAC,
   bad MAC length/zero/multicast/broadcast, duplicate install and lifecycle.
   Close/terminal/profile/link/address/mask changes remove ownership. Elapsed
   time, transient ACK pause and same-address DHCP renewal do not. Old scope
   cannot apply after reconnect; a fresh session can accept a replacement NIC
   MAC. These are design-model checks, not runtime implementation coverage.

Production main.c/net.c, service protocol/transport, SDK etharp.c and original
normal UF2 hashes were checked unchanged before/after. The normal UF2 remains
`ecaad23d53a85ef64b042138ed27c2f7655402c231efaf5d8ff8f17cc4d8516a`.
No Cargo, socket listener/client, USB, SSH, route modification, router login,
SDK mutation, service restart or firmware write was performed.

## Reproducible private evidence

Sources, commands, run logs, model cases and summaries remain in
`/tmp/picomado-server-link-design/`. Copies are preserved privately in
`/home/tane/picomado-staging/20261108-server-link-design/` (directory0700,
files0600). `check.py` builds only temporary host fixtures. Its inputs are the
existing repository/SDK, not a new protocol implementation.

- `legacy-welcome.c`, `live-arp-output.c`, and `check.py` are full fixture sources.
- `model.json` names all45 model checks.
- `results.json` contains all8 compiler/run commands and return codes.
- `summary.json` binds the unchanged source/normal-UF2 hashes. SHA256: `bc6b303fb083be76944f7e4f939456cb6671fca2f6740dad24c1b91eeca2a438`.
- `manifest.json` binds all retained test sources and logs. SHA256: `0997322de1bc4d1880a19b6f918d22ad8bb1174bd784064a36f79ae28b3c6096`.

## Remaining acceptance

The current static-ARP A/B/A proves the pre-TCP intervention only. The Welcome
version still needs production object-aware parsing, serialized host lookup
and publication identity checks, old/new peer-pair tests, add/readback/removal
failure handling, DHCP/profile/NIC lifecycle tests, and a separately authorized
real-device comparison. Host `RTM_GETROUTE`/NIC selection is specified but not
implemented or validated by this firmware-side fixture. A pre-Welcome TCP
failure remains outside the method; it is not solved by these host passes.
