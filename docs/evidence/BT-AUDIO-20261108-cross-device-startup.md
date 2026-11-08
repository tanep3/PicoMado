# BT-AUDIO-20261107: cross-device startup transport stall

Read-only investigation on 2026-11-08 JST, extending
`BT-AUDIO-20261108-followup.md`, the ARP records, and the backpressure record.
Source inspected: main `5407f2d`; this is not a deployment manifest or an
independent acceptance. Hardware operations below were performed by the parent,
not this investigator. No USB, SSH, packet-capture start, network setting,
service, playback, or production-code change was performed by this investigator.

## Latest LCD evidence

Existing `journalctl --user -u picomado`, 12:56–13:03 JST:

- Session2524: initial frame2440066 Receipt at12:56:22.310, then approximately
  96seconds without another application receive before replacement.
- Session2525: initial frame2440070 Rendered at12:57:58.432, then approximately
  74seconds without application receive; stream_write_stall at12:59:12.
- Session2526: initial frame2440077 and Bluetooth complete_success, followed by
  approximately100seconds without application receive before replacement.
- Session2527: frame2440081 writer completion13:01:03.711436, Receipt received
  13:01:09.516, Rendered received13:01:10.657. Up/Down/Sub then arrive together;
  control_write_failed ends the session. Writer completion is not Pico receipt.
- Session2529 from13:02:32: heartbeats and image replies resume; Bluetooth
  complete_success at13:03:07.625. The13:03:14 host TCP snapshot has Send-Q=0.

E-ink heartbeats continued during the earlier LCD stalls. LCD Hello persistence
also took1.07seconds in session2527, and later input persistence took0.46–0.82s.
These are separate host-side delays, not an explanation for74–100s no-ingress.

## E-ink_0 confirms the same failure class

Parent-owned passive capture files:

- `/tmp/picomado-eink-update-observe-20261108-130347/usb.log`
- `/tmp/picomado-eink-update-observe-20261108-130521/usb.log`
- Existing server journal, E-ink_0/incarnation4/session161 and162.

In HOME, conn1 receives frame2440098, completes CRC and issues Receipt at
6.217s device uptime. The display-completion path issues Rendered at13.394s;
its tcp_write and tcp_output both return0. The main-loop display path only
queues this Rendered after the matching display completion. These are software
completion and COPY/output-return facts, not physical appearance or delivery.

The first snapshot has lastack7177 and the server-IP ARP entry pointing to
router MAC10:3c:59:36:fb:2d. Later snapshots remain atlastack7177 while snd_lbb
increases7451→7900→7998→8390 and nrtx increases2→3→4→5. The receiver window
remains64076, so zero receiver window is not shown. tcp_write/output counters
report no error until the later allocation failure. Server frame2440100 still
arrives and completes its display path at37.120/39.629s device uptime. Thus
the MCU application and server-to-device data continue while reverse progress
is missing; this is not explained by E-ink rendering blocking for two minutes.

The ARP snapshot returns to actual host MAC2c:cf:67:11:72:f3 in the later capture,
but lastack still does not advance. MAC restoration alone did not immediately
recover the already-stalled TCP connection. The exact packet emission and
failure location remain unmeasured; nrtx is a retry counter, not RF delivery.

Host receipt of frame2440098 Receipt is13:03:41.962; Rendered finally arrives
at13:05:54.700, a132.738-second interval measured entirely on the host. It is
correctly ignored for display733 after the current display advanced to734.
Seven queued heartbeats and frame2440100 Receipt/Rendered follow within about
40ms. Do not subtract MCU uptime from these host timestamps. The original
Rendered was delayed, not permanently absent, and stale-token checks must stay.

The old session closes at13:05:54.772. New session162/conn3 receives frame2440103
Receipt at13:05:57.162 and Rendered at13:06:02.246. USB records Rendered at
146.504s uptime and lastack=snd_lbb7875, later7973, using the actual host MAC.
Subsequent host heartbeats at13:06:12/27 confirm limited recovery.

## Secondary resource-pressure mechanism

At126.181s, required Heartbeat(kind5),98bytes, fails with ERR_MEM despite
sndbuf10467 and queue10. This excludes byte-capacity exhaustion and the configured
queue limit32 for that call, but does not distinguish heap from segment allocation.
At131.196s the next conn2's first164-byte write also fails, with sndbuf11680 and
queue0. Conn3 subsequently succeeds.

`firmware/lwipopts.h` has MEM_SIZE16000 and LWIP_NETIF_TX_SINGLE_PBUF1.
Pinned SDK `tcp_out.c:tcp_pbuf_prealloc` allocates max_length with that option,
even for a small control; a normal established-MSS allocation can approach1460
payload bytes. Ten retained small notifications can therefore consume most of
the heap. This is a strong code-supported amplification hypothesis, not an
observed allocation-counter verdict. Existing backpressure fixtures do not
constitute measurement of this device's heap.

`net.c:close_unlocked` detaches callbacks and calls tcp_close; successful graceful
close can retain unacknowledged queues until delivery/close completion. Retained
old-PCB resources are a candidate explanation for the new empty conn2's ERR_MEM.
The delayed application batch arriving near teardown is not proof that close
itself caused recovery. lwIP RTO backoff is another delay contributor; exact RTO
values and output destinations were not captured.

## Hypotheses and next discriminating checks

1. Shared ARP/path selection remains the leading comparison, now across LCD
   and E-ink_0. Both HOME and HOME2 previously received actual-host and router
   replies for .120. Different displays weaken a display-specific explanation,
   but do not independently rule out shared SDK/radio behavior or establish
   one AP as defective. Both updated builds include Bluetooth support.
2. Router MAC is not sufficient proof of complete forwarding failure:
   `/tmp/picomado-arp-ping-control.log` has4/4 ICMP replies withTTL254 while
   `/tmp/picomado-arp-ping-after-mac.log` has2/2 withTTL255. Router same-LAN
   forwarding/TCP state and proxy ARP remain unverified.
3. Host eth0=.120 and wlan0=.121 still share192.168.0.0/24; arp_ignore,
   arp_filter and arp_announce read0. This retains the multi-NIC hypothesis,
   but the competing MAC is the router, not host wlan0(:f4).
4. A separately authorized, reversible one-variable experiment could use a
   diagnostic firmware's fixed actual-host MAC for only direct HOME/HOME2
   server .120, established before a fresh TCP connection. Keep AWAY, gateway,
   other peers, application protocol and runtime settings unchanged. Remove
   the entry on profile/interface loss, preserve the prior UF2, test boundaries
   on host first, and compare first receipt/render/heartbeat progress over
   repeated cold starts with unchanged baseline. No such trial is authorized
   or implemented by this record; it cannot be treated as a permanent fix.
5. Separately evaluate failure-only PCB abort/resource release instead of
   graceful close after a terminal local-send failure. Preserve normal EOF,
   display salvage and requested graceful close. First reproduce retained
   old-connection allocation pressure with actual lwIP host fixtures and
   enforce stale-callback/session tests. This is a recovery mitigation, not
   correction of the initial ARP/TCP cause, and is not yet implemented.

Do not weaken stale-result rejection, add background audio/reconnect tests,
globally change ARP/timeout settings, or hard-code a universal next-hop MAC on
the strength of this evidence. A configuration/UI fix for actual proxy-ARP or
duplicate-IP behavior is preferable if subsequent authorized inspection proves
that cause. Independent verification and operational effectiveness remain open.
