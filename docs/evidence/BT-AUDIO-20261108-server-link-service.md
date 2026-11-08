# Welcome server link hint: service implementation and local verification

Change: [BT-AUDIO-20261107](../changes/BT-AUDIO-20261107.md).
Specification: [server link hint](../design/server-link-hint.md), fixed in
`4c2a05c`. This is service implementation evidence, not independent acceptance,
firmware application, network delivery or startup-effectiveness evidence.

## Implementation boundaries

- `TrackedListener` captures concrete local/peer socket addresses at accept
  alongside its existing accept order, using an optional safe TCP type adapter.
  Generic/non-TCP IO retains no endpoint metadata. No fd/address map or
  wildcard listener address is used as a NIC identity.
- Hello admission and `BoundReply` ordering are unchanged. The connection's
  writer prepares the optional hint immediately before Welcome serialization.
  Its current session must have `server-link-hint-v1`. Both outer identity
  fields match that binding; the current App session is checked again after
  the asynchronous lookup. The query never holds an App guard. Later refresh,
  Settings, image and audio work cannot overtake this Welcome.
- Pure admission requires the accepted IPv4 pair, one current eligible address
  owner, a contiguous /1–/30 netmask, both non-network/non-broadcast endpoints
  on that subnet, and a nonzero unicast MAC. Identical repeated interface facts
  are one owner; different-IP aliases do not count as another owner. Conflicting
  facts, another interface owning the same IP, IPv6, loopback, point-to-point,
  down/no-ARP links, unknown or off-subnet peers omit the hint.
- Linux libc `getifaddrs` supplies IPv4 ownership/netmask and current AF_PACKET
  Ethernet MAC. GETLINK rejects logical links, masters, cross-namespace links,
  and links whose iflink differs from their owner. RTM_GETROUTE explicitly asks
  for the accepted peer with accepted local source. It does **not** force OIF.
  Admission requires exact source/destination, RTN_UNICAST, matching owner OIF,
  and no gateway, via, multipath, encapsulation or nexthop-ID result. A second
  interface snapshot must reproduce the same admitted hint after route lookup.
- The per-Welcome lookup budget is 100 ms from scheduling through serialization
  eligibility. Nonblocking netlink sockets use bounded poll and datagrams,
  validate kernel sender/sequence/type/length, and reject multipart/truncated
  or ambiguous data. At most four blocking lookups run or remain queued; a
  timed-out worker keeps its permit until it really exits. Unsupported OS,
  saturation, syscall errors, timeout or stale result omit only the hint.
- These are bounded current OS snapshots, not an atomic lease on future kernel
  topology. Serialization and local write acceptance do not prove remote
  delivery. No timestamp is sent to Pico. No learned-neighbor MAC, process-wide
  MAC cache, durable hint, subprocess, router access or network mutation exists
  in the connection path.

## Focused checks

Pure fixtures exercise unique/ambiguous owners, aliases, invalid masks/MACs,
loopback/IPv6/off-subnet endpoints, wrong peer/OIF, gateway/via/multipath and
logical-link rejection, malformed/truncated netlink messages, and expired
lookup deadlines. Protocol checks round-trip old/new Welcome and preserve the
exact old Welcome JSON when the optional field is absent.

The production publication adapter fixture proves old capability sets never
call the resolver and produce the identical encoded Welcome; missing metadata
and failed/stale resolution preserve ordinary Welcome. A real App owner can be
acquired inside the resolver future. Replacement by a newer Hello while that
future runs returns `session_not_current`; a reply from another binding is
rejected before lookup. Existing accept-order and Welcome-before-outbox tests
remain regression coverage rather than being replaced by a new model.

## Read-only Linux adapter check

On raspi5, the explicit ignored `server_link::tests::live_linux_lookup` test
queried accepted local `192.168.0.120` and peer `192.168.0.44`. It exercised the
actual getifaddrs → GETLINK → GETROUTE → getifaddrs path and returned:

```json
{"version":1,"server_ipv4":"192.168.0.120","client_ipv4":"192.168.0.44","prefix_length":24,"mac":"2c:cf:67:11:72:f3"}
```

The kernel reply has source/destination lengths32, RTN_UNICAST, OIF2 and no
gateway. The test completed in under0.01s. Initial sandbox netlink socket access
returned EPERM; the supported approval/escalation route allowed the read-only
local-kernel test. No packets to the Pico, network changes or SSH were used.
This demonstrates that the conservative parser produces a real hint on this
host rather than passing synthetic cases while always omitting production data.

## Aggregate check limits

The optional project gate was run with a fresh output directory. Its structural
suite passed48 of49 cases. `test_gate_routes.Routes.test_actual_firmware_build_sdk_flags_and_offline_flash`
failed at line46: after removing a fixture approval, the build command returned0
where the test expected a refusal. The service changes do not touch the build
route or approval rules; this unrelated assertion was not weakened. Full gate
success and independent approval are not claimed.

Full server regression and final commit results are recorded below after the
run completes. Deployment/release artifact and silent device trials belong to
the parent integration task. No service install, device operation, playback,
router login, network configuration change, SSH or push occurred in this worker.
