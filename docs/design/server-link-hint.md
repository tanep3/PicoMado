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

Equal prefixes do not prove physical L2 reachability or exclude proxy ARP. The
existing HOME hardware comparison supplies evidence only for that tested path.

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
