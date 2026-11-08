# Server-provided link hint: firmware implementation and offline evidence

Contract: `4c2a05c`, [server-link-hint design](../design/server-link-hint.md),
under BT-AUDIO-20261107. User approved HOME/HOME2 application, not AWAY/router
changes. This worker implements/builds but does not operate USB or flash.

## Firmware

`PM_SERVER_LINK_HINT` defaults OFF. The approved candidate enables it and the
SDK's `ETHARP_SUPPORT_STATIC_ENTRIES`; CMake rejects simultaneous hardcoded
static-ARP diagnostic. Product code contains no fixed device ID, IP or MAC.
HOME/HOME2 advertise `server-link-hint-v1`; AWAY does not. The current SESSION's
first Welcome is parsed into temporary values, optional hint applied through
the locked network owner, and only then ACTIVE is published. Old Welcome and
invalid/missing optional hint retain ordinary operation. Duplicate/late Welcome
outside SESSION cannot replace the mapping.

The allocation-free parser has a1024-byte input limit and nesting bound6. It
uses object spans, not global substring lookup, for envelope/data/hint fields;
checks integer overflow, duplicate required/known keys, IP/MAC encoding and
version. Malformed JSON/base identity is rejected; valid JSON with unsupported
or invalid optional hint is ignored. Escaped required-key spelling is not
accepted as an alias for the protocol's literal ASCII names. Unknown fields
cannot supply a nested incarnation/session. Locally administered unicast MACs
are permitted; zero/multicast/broadcast MACs are not.

The network owner rechecks connection/generation/profile/netif/local IP/peer
IP/mask/route under the existing CYW43 lock. Installation/readback precedes later
application sends. The same owner removes at close/terminal failure, observed
link/address/mask/route/netif change or lost table ownership. ACK progress and
elapsed time are not validity conditions. Missing/dynamic removal results are
safe; unknown errors retain ownership and block a new connection/profile.
No persisted setting or SDK source is changed.

## Executed focused host checks

`python3 firmware/tests/run_server_link_hint_tests.py --out
/tmp/picomado-server-link-host-20261108` passes28 compile/run/configuration steps
with native and ASan+UBSan variants. LeakSanitizer is disabled under the executor.

- Actual parser/domain: old/optional/null/invalid cases, direct required keys,
  escaped spelling, triple duplicate hint, nested false identities, overflow,
  every truncation byte, depth/size bounds,5000 bounded malformed mutations,
  endpoint/prefix/profile/connection/generation/MAC checks.
- Actual production net owner: post-connect apply under lock, HOME/HOME2,
  duplicate apply, stale query/reconnect, link/IP/mask/netif change, same-address
  renewal,1000 polls with unacknowledged bytes, SDK-owned PCB failure, existing
  terminal cleanup, add/readback failure and unknown cleanup failure.
- Actual main: capability only on direct profiles, old/new/invalid Welcome,
  apply while still SESSION before ACTIVE, failed optional apply fallback,
  duplicate ignore and BT-disabled integration, both feature OFF and ON.
- Existing ordinary network and terminal cleanup tests pass unchanged.
- CMake default/enabled accepted, diagnostic conflict rejected, invalid C option
  rejected. `git diff --check` passes.

An initial app fixture run omitted the fake transport READY setup before Hello;
that fixture precondition was corrected without changing product behavior or
weakening assertions. Final results are in `results.json`/`summary.json` and
per-step logs in the above private directory.

Full root/Cargo validation belongs to the integrating parent and service worker.
Regular ARM artifact hashes and hardware acceptance are separate follow-up
records. The Welcome feature cannot repair failures before Welcome arrives and
is not cryptographic authentication. No RF/physical effectiveness is claimed by
these host passes.
