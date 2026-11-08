# Remembered server MAC: service claim comparison

Change: BT-AUDIO-20261107. Specification: `ef62c27`, final cache section in
[server link hint design](../design/server-link-hint.md). This complements the
existing accepted-socket Linux hint adapter; it does not persist a MAC on the
server or change the network, route lookup, Welcome fields, or lookup budget.

## Query, comparison and publication

A Hello advertising `server-link-cache-v1` can include `remembered_server_link`
with `version:1`, `server_ipv4`, and colon-separated six-byte unicast `mac`.
The compact validated claim is carried only with that connection's first bound
Welcome reply. It is never put in App/SQLite and cannot select an interface,
route, destination or hint value. The independent current lookup still takes
only the accepted socket endpoints. Existing current-session fencing and
Welcome-before-output ordering remain in force.

A flattened, bounded-by-the-existing-control-frame parser preserves duplicate
map entries instead of normalizing them through a JSON object map. Repeated
optional fields, duplicate/unknown inner fields, unsupported versions, invalid
IPv4/MAC and wrong-shaped optional values become an invalid claim without
rejecting an otherwise valid Hello. Missing/null is absent; unrelated unknown
Hello extensions retain prior ignored behavior. Required Hello fields and the
existing4096-byte control cap remain strict. The absent field serializes to the
exact old Hello shape, and Welcome wire behavior is unchanged.

One bounded diagnostic per accepted cache-capable Hello reports
`picomado server_link_cache ... stage=welcome_prepared remembered=... comparison=...`.
`remembered` is `absent`, `invalid` or `value`; `comparison` is `new`, `matched`,
`mismatch`, `invalid` or `unavailable`. A valid claim for another server IP is
invalid for this comparison, not a mismatch against another NIC. MAC comparison
uses bytes, so case differences do not count as changed memory. Unnegotiated
claims produce no comparison diagnostic. No raw claim, MAC, SSID or credentials
are logged. This is a preparation/comparison fact, not delivery or durable-save
confirmation; the Pico owns its own validated replacement and persistence.

A missing or invalid claim never supplies current facts. When the independent
current lookup succeeds, the normal current Welcome hint can still be sent.
When current lookup is ambiguous, unsupported, stale, failed or unavailable,
there is no hint and therefore no current value to save.

## Verification

The frozen implementation passed31 focused checks with0 failures:

- `cargo test --manifest-path service/Cargo.toml -j1 --lib server_link`:
  15 passed,1 intentionally ignored local-kernel test. This includes the actual
  serde/WireMessage/Decoder duplicate-key, invalid optional, unknown/nested,
  size-limit and old-JSON roundtrip fixtures; capability gating; current-hint
  authority and connection-local BoundReply; the existing route, timeout and
  stale-session checks. Compile plus prior Cargo-lock wait took6m52s; the
  focused checks took0.13s. No source changes followed the run.
- The same resulting test binary ran all7 protocol tests, all8 Hello accept-order
  tests, and the existing Welcome-before-outbox regression. All passed. These
  use temporary fixtures and in-memory transport; no production connection was
  made. The Linux lookup implementation is unchanged and its separate live
  test was not rerun as part of this claim-only extension.
- `git diff --check` passes. The unrelated music changes were not edited or
  included in this worker's commit. The final test binary was made available
  to their owner for separate music checks.

Retained log: `/tmp/picomado-remembered-link-focused.log`, SHA256
`91bfbd746c627d16f36cc1ea67b9dbf3f02597db76207f1c744f41639dc77b22`.
Additional logs: `/tmp/picomado-remembered-link-protocol.log`,
`/tmp/picomado-remembered-link-hello-order.log`, and
`/tmp/picomado-remembered-link-welcome-order.log`.

The parent coordinates the combined full Cargo regression, production release,
USB and device trials. No service deployment, device/flash operation, router or
network change, SSH or push is performed by this service implementation worker.
