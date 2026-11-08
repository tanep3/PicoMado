# Session-scoped server link hint — design only

Change ID: [BT-AUDIO-20261107](../changes/BT-AUDIO-20261107.md). That problem
record owns the requirements and causal history; this document is its proposed
protocol/lifecycle design, not an implemented runtime contract or deployment.
No firmware/service behavior, network setting, USB or flash operation is part
of this design task. The separate static-ARP diagnostic remains unchanged.

## User proposal and evidence

The user proposed that the server tell the Pico its own MAC when connecting,
so the Pico can select it without a hardcoded IP/MAC pair. The reversible HOME
normal → fixed → normal test supports this direction: the normal trials stalled
at ACK7177 and recovered on connection3, while the fixed trial stayed on
connection1 with every retained ACK caught up. See
[the qualified hardware evidence](../evidence/BT-AUDIO-20261108-home-static-arp.md).
This supports the selected ARP path as a contributor, not every possible cause.

The observed result was **pre-TCP hardcoded** pinning. Receiving a MAC later in
Welcome is a different intervention and still needs its own controlled trial.
It must not be reported as already proven to repair the startup failure.

### Actual ordering and timing bounds

1. IP assignment, TCP handshake, HTTP WebSocket upgrade.
2. `main.c` sends Hello only after `PM_NET_READY`, then enters SESSION.
3. `transport.rs:process_inner` accepts Hello and binds a fresh server session.
   `WriterCommand::BoundReply` schedules Welcome ahead of refresh/output work.
4. `main.c:api_welcome` accepts only in SESSION, parses incarnation/session,
   enters ACTIVE, then displays local status. Settings/frames/BT commands follow.
5. Initial Receipt and later Rendered/status writes follow those received frames.

Normal-before capture153352 received valid frame2440242 at MCU11.299166s,
so Welcome had already been processed by then; the exact Welcome timestamp is
not logged. Its first retained snapshot already had pending bytes. In restored
capture154824, valid frame2440245 began at11.222738s, the roughly15s snapshot
still had ACK=send_end=7177, and Rendered was submitted at17.866280s before the
later fixed ACK was observed. Thus an in-Welcome update can precede the later
observed stall in these trials. We cannot recover exact lost-packet/ACK onset
from15-second USB snapshots, and write/output success is not delivery.

A Welcome hint depends on TCP/HTTP/Hello succeeding. It cannot repair initial
SYN, Upgrade or Hello failure before the hint arrives. It may also arrive after
lwIP has already generated a TCP ACK for the Welcome packet; the guarantee is
about subsequent application sends, not every earlier transport packet. A
larger Welcome changes packetization, so this timing hypothesis requires a new
trial even though its ordering is promising.

## Proposed v1 wire behavior

- New Pico advertises capability `server-link-hint-v1` for HOME/HOME2 direct
  connections. Old Pico does not advertise it and gets unchanged Welcome.
- Welcome gains optional `server_link` object, version1: `server_ipv4`,
  `client_ipv4`, `prefix_length`, and six-byte unicast `mac`. These addresses
  describe the **accepted connection**, not an arbitrary configuration target.
- Outer Welcome incarnation/session remain the session identity. No new message
  kind, persisted setting, device-specific literal IP/MAC, UDP discovery or
  pairing credential is introduced in v1.
- New Pico with old server or omitted/unavailable/unsupported hint follows
  ordinary dynamic ARP. Invalid optional hints are rejected with a bounded
  diagnostic; they do not invalidate an otherwise valid Welcome.
- New server omits the field for old capability sets. Existing serde Hello
  capabilities already admit new bounded strings. The old Pico ignores unknown
  Welcome members; an offline production-handler fixture verifies this.
- Parse the new object with a bounded object-aware parser. Existing `number()`
  uses a global `strstr` and is not suitable for safely extracting nested hint
  fields or rejecting duplicate keys. Do not duplicate incarnation/session
  keys inside the new object or silently widen current message-size limits.

## Host-side authority and freshness

The host-side investigation owns the OS lookup details. Carry accepted socket
`local_addr` and `peer_addr` through TrackedListener → WriteProgress/ConnectInfo
→ transport; a listener bound to0.0.0.0 is not itself a concrete NIC address.
Resolve the accepted local IPv4 to exactly one current eligible interface and
its current MAC/netmask, outside the App lock, using existing libc/getifaddrs.
No shell commands, ARP-neighbor learning or router login belong in this path.

The candidate must be from that accepted connection, still current at Welcome
publication, and fresh under a short **host-local lookup deadline**. Never send
or compare host monotonic timestamps against Pico uptime. Omit the hint on
lookup failure, stale result, IPv6, loopback, non-Ethernet link, multiple owners,
invalid MAC, missing mask, a nonlocal peer, or an ambiguous route/interface.
Do not persist the candidate in App/SQLite or reuse a process-wide MAC cache.
In a generic listener, use an optional connection-metadata adapter or a concrete
TCP wrapper; do not call TcpStream-only methods on unconstrained generic IO.
Do not use a global fd-to-address map, because descriptor reuse breaks identity.
A result from an obsolete accept-order/session cannot annotate a newer reply.
Existing `hello_accept_order` rejects an older accepted connection's late Hello;
publication must retain the same binding and not relabel delayed lookup results.

The DHCP server is the user's LXD dnsmasq on192.168.0.160, not L13; L13 DHCP is
reported disabled. Therefore require **both endpoints' prefix agreement**,
not merely Pico's DHCP mask. Host accepted local/peer must share its actual
interface prefix; the proposed hint carries that prefix and both accepted
addresses. Pico requires an equal current contiguous mask/prefix, matching own
IPv4 and server peer IPv4, plus direct same-netif routing. Different/unknown
prefixes omit/reject the hint. This deliberately rejects some legitimate but
complex configurations instead of pinning a gateway as a peer.

Prefix equality alone does not exclude a more-specific host/policy route.
Before publishing, the host additionally queries `RTM_GETROUTE` for destination
accepted peer IPv4 with source accepted local IPv4. Require `RTN_UNICAST`, no
gateway and output ifindex equal to the uniquely selected address-owner NIC.
Unknown, stale, unsupported or failed route results omit the hint. Current
getifaddrs ownership alone is not a general proof of the route. Initial support
omits point-to-point /31-/32 and complex/ambiguous bridge, bond, VLAN, VRF or
tunnel configurations; future support must select the IPv4-owning logical link,
not a guessed physical slave. This is separate from MAC validation: a valid
locally administered unicast MAC is not rejected merely for lacking a global OUI.

Even these route facts do not prove physical L2 reachability or exclude proxy
ARP. The existing HOME hardware comparison supplies evidence only for that
tested path.

## Pico query → decision → effect and ownership

The current network aggregate owns at most one RAM-only pinned mapping:
`{connection_generation, link_generation, profile, netif, local_ipv4, mask,
peer_ipv4, incarnation, session_epoch, mac}`. There is no globally authoritative
MAC and no use of an old hint to bootstrap the next connection.

At the **first** Welcome for the current SESSION:

1. Validate outer Welcome into temporary values. Capture the current transport
   identity and local route facts, not a host-reported claim that the link is up.
2. A pure rule accepts only HOME/HOME2, non-SOCKS IPv4, current READY connection,
   UP/link-UP interface, assigned local IP, valid contiguous matching prefix,
   peer on that subnet, hint server IP equal to the actual PCB remote/configured
   target, hint client IP equal to current local IP, and nonzero/nonbroadcast/
   nonmulticast six-byte MAC. Locally administered unicast MACs remain valid.
3. Under the existing CYW43/lwIP lock, recheck connection/link/profile/netif/
   address/mask identity. A stale query cannot authorize the effect. Add via
   actual SDK `etharp_add_static_entry`, verify readback, and bind ownership to
   this Welcome/session. No TCP close/reconnect is needed merely to change a
   live connection's ARP mapping. Subsequent Ethernet output consults that map.
4. Only then enter ACTIVE and run local status, SettingsStored, Receipt/Rendered
   or BT work. Do not wait for an extra application ACK to apply the hint: that
   would recreate the dependency the feature is trying to remove.
5. Missing/invalid/unavailable add/readback yields ordinary operation, explicitly
   marked not pinned. Clean up any partially owned pin; unexpected cleanup
   failure must not leak it into another profile or be reported as success.

Ignore duplicate or late Welcome while ACTIVE. An asynchronous apply/query is
bound to the original connection and join/profile generation, never merely the
same numeric session from a later boot. Reuse the actual network owner APIs,
not USB state or a new parallel networking authority.

### Lifetime and changes

- Keep the RAM mapping for the current successful connection. **No fixed TTL
  that returns a healthy long-running session to competing dynamic ARP.** The
  host lookup freshness deadline above is not the installed mapping lifetime.
- Remove on close, terminal failure, profile/Wi-Fi-link change, or local
  address/netmask/netif identity change. Same-address DHCP renewal keeps it.
- On retry, begin with ordinary ARP and obtain a fresh Welcome. Close/removal is
  idempotent and under the SDK lock. Do not abort a connection solely because
  its ACK is briefly unchanged; use the existing timeout/terminal policy.
- NIC replacement/MAC change during a connection may temporarily break that
  connection. Existing failure/reconnect drops the old RAM pin and obtains the
  new MAC. Do not cache the old pin across reconnect/reboot, which could block
  the very Welcome needed to correct it.
- A changed server IPv4 is a separate endpoint-discovery/configuration problem;
  a Welcome at an old unreachable address cannot discover the new address.

## Trust boundary and exclusions

Current HOME uses plaintext `ws://` with WebSocket Accept checking, not TLS,
server signatures or mutual authentication. A configured IP, established TCP,
incarnation, session, accept-order or nonce is **not cryptographic identity**.
The hint is accepted only from the current configured server's application
connection under the existing trusted-LAN assumption. Scope checks prevent
accidental cross-profile/target mapping but do not defeat an on-path attacker.
Do not present this change as ARP-spoofing protection or add credentials silently.

Exclude AWAY/SOCKS, remote routed peers, differing/unknown subnets, IPv6,
ambiguous host interface ownership, and unsupported old peers. Do not alter
router configuration; the user declined router login. SSH remains prohibited.

## Bootstrap alternative and acceptance

Do **not** start with an additional discovery mechanism. First test the simpler
Welcome-scoped proposal, because these failing sessions had reached Welcome.
If a capture proves failure before Welcome, obtaining a MAC before TCP requires
another reachable trusted path or provisioned data; it cannot be achieved by
moving a field within the same never-established TCP stream. UDP/broadcast
challenge discovery would need its own bounded timeout, collision/identity and
trust design. A previously learned permanent MAC has a stale-NIC bootstrap
failure and is not the proposed fallback.

Offline work here tests a pure proposed decision/lifecycle model, the current
production Welcome handler's additive compatibility and state ordering, and
actual installed SDK Ethernet output before/after a live mapping replacement.
It does not implement the wire feature or prove RF delivery. Future runtime
acceptance must cover old/new peer pairs, malformed/duplicate/nested fields,
size limits, late lookup and reconnect races, profile switching, DHCP changes,
NIC replacement and SDK add/readback/removal failure, followed by same-device
ordinary → Welcome-hint → ordinary silent trials with exact receive/apply
markers, ACK progress, Receipt/Rendered and drop accounting.

## Authorized v1 implementation contract (before implementation)

The user approved implementation and application of HOME/HOME2 temporary MAC
selection with cleanup on disconnect; AWAY/router settings remain unchanged.
This section promotes the preceding design into the implementation contract,
without claiming it is already implemented, deployed or effective.

Wire names are agreed between firmware and service implementers:
`server-link-hint-v1`; optional Welcome `data.server_link` object containing
`version:1`, dotted-decimal string `server_ipv4`, dotted-decimal string
`client_ipv4`, integer `prefix_length`1..30, and colon-separated six-byte `mac`.
Hex case is immaterial. Outer Welcome incarnation/session bind the hint; never
repeat those keys inside the object. Omitted or null means no hint. Invalid
optional contents mean hint rejected, with a valid ordinary Welcome retained.
Invalid required Welcome envelope/session fields still use normal failure.

For staged firmware rollout, `PM_SERVER_LINK_HINT` defaults OFF; this approved
E-ink_1 candidate explicitly enables it. Enabling it also enables the actual
SDK `ETHARP_SUPPORT_STATIC_ENTRIES`. Reject a build combining it with the older
hardcoded `PM_HOME_STATIC_ARP_DIAGNOSTIC`. No hardcoded IP, MAC or device identity
belongs in this product feature. HOME/HOME2 advertise capability; AWAY does not.

The product parser is allocation-free, bounded by the existing control-message
size and a small nesting limit. It validates direct envelope/data fields,
integer overflow, exact IPv4/MAC encodings and duplicate required/known fields.
Optional hint data is not parsed with the old global substring helpers. Parse
base session data into temporary values; apply a validated hint through the
network aggregate before publishing ACTIVE or starting later application sends.

Network query/apply snapshots bind transport generation, profile, link/route
identity, local/remote IPv4 and mask/netif. Recheck under the existing SDK lock.
The owner keeps one current-session RAM mapping. Close/retry/terminal failures,
observed link/address/mask/route changes revoke it; no normal-session TTL and no
ACK-stall-triggered reset are added. Missing/invalid hint or add/readback failure
continues unpinned ordinary operation after safe cleanup. Unknown cleanup failure
retains ownership and prevents leakage into a different connection/profile.

Service owns socket-local accepted metadata and asynchronous bounded lookup
outside the App lock. Publication must keep the original accepted connection
and session association. Linux eligibility requires unique getifaddrs IPv4
owner plus RTM_GETROUTE(peer, source=local): unicast, no gateway, OIF matches
owner and same prefix. Failure/ambiguity/deadline omits the optional field,
not Welcome. The firmware independently verifies its live endpoint/prefix.

Acceptance includes new parser/domain/owner/app host fixtures, unchanged old
paths and BT-OFF, service old/new negotiation and bounded OS-adapter fixtures,
regular E-ink_1 ARM build with source/config/SDK/artifact hashes, then parent-owned
same-device silent application/comparison/rollback. Native/sanitizer/model passes
are not physical acceptance. Root-wide Cargo/build validation remains parent-
coordinated to avoid duplicate heavy jobs. Workers do not flash or use SSH.

## Remembered MAC extension — approved contract, not yet implemented (2026-11-08)

Change ID remains BT-AUDIO-20261107. The user proposed retaining the previous
IP/MAC and updating that memory when the host detects a mismatch, then explicitly
approved implementation and E-ink_1 testing of per-HOME/HOME2 persistence,
pre-TCP reuse, finite ordinary-ARP fallback and correction from the current host.
This section supersedes the earlier v1 prohibition on Pico persistence only for
this approved extension. It does not claim implementation, application or repair.
The host still resolves its current accepted interface independently every time.

The Welcome-only trial180258 installed the expected MAC at25.733s, reached
ACK=send_end7199, then stalled after later sends. The pre-TCP fixed comparison
181500 succeeded across retained snapshots despite a Wi-Fi retry/if2; restored
Welcome trial181756 stalled again. Parent owns the complete hardware evidence.
These results prioritize pre-TCP reuse; they do not prove a universal cause or
that remembering a MAC will remedy every connectivity failure.

### Connection and update rules

- Keep separate records for immutable HOME0 and HOME2=2. AWAY1 neither reads,
  installs nor updates them. The key is exact profile ID, configured SSID bytes
  (maximum32, no password) and configured server IPv4. Store only that key, the
  validated prefix1..30 and valid six-byte unicast MAC. Do not store client DHCP
  address/session as reusable identity; same-subnet client address changes are
  allowed after checking the new live network. Locally administered MACs remain
  valid. The key prevents accidental mixing, not attacks or identical-name LANs.
- Before TCP allocation, query/recheck current UP/link-UP IPv4 interface, direct
  non-SOCKS route, configured target, same subnet and matching stored prefix.
  Install/read back a matching remembered value under the existing SDK lock;
  bind the effect to the new connection/profile/link scope. Never use a gateway
  mapping as the server. Ineligible/missing/add/readback failure starts ordinary
  ARP only after safe cleanup. Unexpected removal failure remains fail-closed.
- A cache-seeded connection is provisional until its first valid Welcome. The
  existing nonextending SERVER+SESSION app_s deadline covers TCP, WebSocket and
  Hello/Welcome (default10s, configurable3..120s). A failed cached attempt marks
  that exact value blocked in RAM for this boot; the next existing retry starts
  without it. Re-JOIN/profile switching does not reset the block. Terminal
  failure of the seeded session likewise forces an unseeded next attempt.
  This bounds cached attempts, not the time until a reachable server succeeds.
- Current valid Welcome replaces/refreshes the current mapping, even when a
  provisional cache owner already exists. Revalidate profile/connection/link and
  endpoint facts before effect. Valid changed data updates RAM immediately and
  marks a durable update pending. The same value never causes a flash write.
  Failed/stale/ambiguous/invalid hint never replaces durable memory. Missing or
  invalid optional hint releases a provisional owner and retains old-server
  compatible operation. Duplicates/late ACTIVE Welcome remain rejected.
- Preserve no-TTL healthy-session ownership and existing terminal/close/link
  cleanup. Do not add ACK-stall resets. A blocked exact old value remains blocked
  this boot; a different validated value can be used on a later connection.
  A fresh boot may attempt its durable value once again. Persist no failure
  counter, last-used time, retry status or other per-connection flash churn.
- Changed configured server IP/SSID means the old entry is ineligible; learn at
  the new target. NIC replacement at the same IP can recover through the finite
  ordinary-ARP attempt. This is not discovery of an unknown changed server IP.
  Plaintext Welcome/session identity is still not cryptographic authentication.

### Additive Hello contract agreed with the service implementer

HOME/HOME2 advertise both `server-link-hint-v1` and `server-link-cache-v1`.
Hello data can include optional `remembered_server_link` containing `version:1`,
`server_ipv4` dotted string and six-byte colon-separated `mac`. It describes only
valid memory for the current configured profile/key, including a value currently
blocked from bootstrap, never the other profile. No match means omit the field.
The server compares this claim with independently acquired current accepted
local NIC facts and emits at most one bounded matched/mismatch/absent/invalid/
unavailable diagnostic. The claim must never select its NIC/route/hint value.
Invalid optional data is ignored without breaking a valid Hello. The existing
Welcome `server_link` current value is returned unchanged; no new Welcome status
field is needed. Capability absent/old peer preserves previous behavior. Pico
performs its own comparison and does not trust a host comparison label.

### Smallest independent storage and image boundary

The approved initial choice is **one4KiB sector at physical FLASH_SIZE-16KiB**.
The preceding8KiB BT store remains at FLASH_SIZE-12KiB; the final4KiB settings
store remains at FLASH_SIZE-4KiB. Do not change their offsets, formats, IDs or
contents. The legacy settings struct is56bytes with CRC at52 (`main.c:395–419`,
`home2_profile_test.c`); its writer erases its whole sector. Merely extending it
would let MAC learning erase unrelated profile/timeouts and makes old writers
remove the extension. BT TLV sharing also couples this feature to BT_OFF and
link-key storage. An independent cache has a smaller failure scope.

Use a small append-only sector, not a general NVS framework: explicit byte order,
magic/schema/length, bounded HOME/HOME2 snapshot, sequence and CRC in one256-byte
page, with a separate256-byte commit-marker page and verified readback. Program
pages only once, append while erased space exists, erase only this sector when
full. Interrupted erase can lose the entire cache: this is explicitly allowed
and falls back to normal ARP. A two-sector journal is unnecessary unless retaining
an old cache through every erase becomes a requirement. Reject corrupt/unknown
records without writing defaults. Do not wrap an exhausted sequence.

RAM learning and durable saving are separate effects. Queue/coalesce the newest
changed snapshot, suppress unchanged writes, and rate-limit commits (minimum60s
between successful commits). Execute from main outside all network callbacks and
lwIP locks, after the immediate Welcome/startup work, with no active audio or
panel operation. Return/log safety-entry, erase/program/readback/commit outcomes;
retain RAM/pending state on failure. Use finite SDK flash-safe admission with
verified core/XIP ownership. IRQ blackout and actual erase time are not bounded
by that admission timeout and need hardware acceptance. Do not assume the old
settings writer or BT backend's unchecked infinite admission establishes safety.

Reserve final16KiB in the **linker FLASH region** and reject UF2 blocks overlapping
it, while keeping physical `PICO_FLASH_SIZE_BYTES` unchanged. The installed SDK's
`pico_override_flash_size` also changes that C macro and would shift old storage,
so do not use it. Use its target linker-script override path for a reduced
`pico_flash_region.ld`, plus physical-offset assertions and complete UF2 checks.
Current hint/original images end at0x10087f00/0x10085c00, below new start0x101fc000
for this2MiB target. This proves these images have no payload there, not arbitrary
future/old images. The parent flash helper uses `picotool load -v -f` without a
whole-chip-erase option, and old runtime writers target only settings/BT. Actual
reserved-region preservation across deployment still requires readback acceptance;
no such device read or mutation occurred during this design task.

### Prior pure-model evidence and remaining acceptance

The earlier two-sector **proposal model**, not product code, ran8 Python tests:
513 append byte cuts,4609 rollover byte cuts,104 corruption locations, schema/
sequence conflicts, profile/key fences, dedup, replacement and finite fallback.
All passed. This proves only that model's assertions, not the new one-sector
implementation, flash safety/endurance or RF recovery. Private retained source:
`/home/tane/picomado-staging/20261108-server-link-cache-design/` with `design.md`,
`model_test.py`, `model-test.log`. SHA256 respectively:
`e47dfb6b86e2df10af1592596cc75d4d7f4049d5f22b7d6bc2f44e9bbc0c1d9d`,
`6a08a0fcdda9f4a9d9c5720572db827305decfafa91aa63ab9671e6e0c5892cc`,
`8eaac8876997ca6290b9ee516bbc646ae52eb4074415933ab101214093e99cc7`.
Its two-sector recommendation is superseded by the explicit one-sector choice
above; it is retained as design history, not silently relabeled as final testing.

Implementation must add actual codec/storage and owner/app tests, every one-sector
power-cut boundary and out-of-sector preservation, old settings/BT_OFF regression,
stale-NIC fallback, old/new Hello compatibility, linker overflow/UF2 negatives,
and regular serial ARM build. Parent owns finite silent seed→reboot/pre-TCP-use→
change/fallback comparison, original-artifact rollback, reserved-region readback
and independent acceptance. No storage feature is considered shipped by this
specification commit or by a model pass.
