# Audio-status send backpressure (host evidence)

Change: BT-AUDIO-20261107. Specification fixed first in `f2fcc9b`.

## Finding and limitation

Saved L13 trial ends with stream 2 accepted=4096, consumed=3456 and
`net tcp_write_failed sdk_result=-1`, followed immediately by transport FAILED.
`net.c` tested only available send bytes, while installed lwIP `tcp_write`
also rejects the queued-pbuf limit and allocation failures before committing
COPY. With current options, the queue bound is 32 and TCP segment pool is 32.
The saved log does not identify the message kind or exact exhausted resource.
It is not evidence that the network or ARP was healthy.

The existing active audio bridge sent identical absolute credit every 10 ms.
The new path sends changed/start/end/failure status only, retaining latest
absolute accepted/consumed values after WAIT in `bt_media`. Attempts are at
least 10 ms apart, with a non-extending 1500 ms first-WAIT deadline. Scope is
both the existing media identity and transport connection ID. A new connection
cannot accept an old report or be failed by its stale timeout. Ordinary
required-control COPY semantics, framing, SDK/options and ARP are unchanged.

General required controls still fail explicitly on exhaustion. This scoped fix
cannot establish recovery if the original failed message was such a control.
Failure logs now include kind, byte length, send capacity, queue count and
retryability so the next bounded hardware trial can distinguish them.

## Executed checks

`python3 firmware/tests/run_backpressure_sdk_tests.py --sdk <installed-sdk>
--out <temporary-results-directory>`: all 12 compile/test steps passed.
Native and ASan+UBSan variants cover:

- Real unmodified installed `tcp_write`/`tcp_output`: insufficient sndbuf;
  independently injected pbuf allocation and segment allocation failures;
  32 tiny unacknowledged writes exhaust queue while byte capacity remains;
  COPY positions stay unchanged on ERR_MEM; releasing one segment permits
  exactly one successful COPY. Allocation, link and ACK release are fixtures,
  not claims about actual radio behavior or real ACK processing.
- Production net adapter: WAIT does not fail transport or call output;
  output failure after COPY stays COPIED; fatal write errors stay FAILED;
  closed/draining and reconnected scope refuse; stale timeout cannot fail a
  new connection. Ordinary HOME/AWAY handshake, controls, keepalive, pong,
  reconnect and failure tests still pass.
- Production audio owner: latest coalesced credit including consumption,
  no unchanged duplicates, 10 ms attempt rate, retry success, finite deadline,
  no replay after terminal failure, new stream, cancellation, stale session,
  display, incarnation, inactive app state, and monotonic-millisecond wrap.

Results: `/tmp/picomado-backpressure-host-final/summary.json` and per-step logs.
SDK source hashes before/after match. Installed `tcp_out.c` SHA-256:
`f8d44fafd6f9dc81442317f9d196aec772f37e02bfdb8ef601601a73454e7660`.
LeakSanitizer cannot run under this executor's ptrace; it is explicitly
excluded. AddressSanitizer and UndefinedBehaviorSanitizer passed.

Root gate did not pass. Its structural run at
`/tmp/picomado-backpressure-gate-check-1/structural.log` found the existing
public-export home path in `cyw43_sleep_test.py` and the offline flash approval
fixture expecting rejection but receiving exit 0. Other workers/parent own
those findings. Full integrated host tests and target build are handed to the
HOME2 integration worker after source freeze; they are not represented by the
12 focused steps above.

No physical device, service, SDK source, SSH, network settings or push was
used. Independent acceptance, integrated build, operational application and
hardware effectiveness are pending with the parent.
