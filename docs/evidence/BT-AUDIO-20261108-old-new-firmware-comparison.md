# BT-AUDIO-20261107: old/new E-ink_1 firmware comparison

Investigation on 2026-11-08 JST, source main `5407f2d`. Extends
`BT-AUDIO-20261108-cross-device-startup.md` and
`BT-AUDIO-20261108-followup.md`. This investigator performed offline reads and
this documentation edit only. Device observation, backup and restart below were
performed by the parent; no firmware write was made for the old-image trial.
This record is not independent acceptance or operational closure.

## Original comparison and result

The user's comparison was: 「初回パケづまりは、以前のファームではなかった現象だ。
差分を比較して、原因調べようぜ。」 E-ink_1 was connected for that investigation.
The previously working artifact must be identified before attributing the
change to Bluetooth, SDK, rendering, or the external network.

**The actual older E-ink_1 program also exhibited delayed/batched replies in
today's environment.** New firmware changes are therefore not necessary for
this failure class. This does not establish that old and new failures have the
same immediate cause, that their incidence/severity is equal, or that the new
firmware cannot amplify an existing fault. The current server and network are
not the complete historical environment. Host/server regression and changed
traffic timing remain comparison axes; an AP-only explanation is not proven.

## Identify the actual old program

- Device: E-ink_1, serial `E6614864D34F8E22`.
- Existing build `firmware/build/E-ink_1/E-ink_1.uf2`: SHA256
  `a2f191ee60e4a5819e20f831f6f44e0fbb50cacb067be8e13e10939fc871e583`.
- The adjacent manifest still names `397115122bca8c1e6db781ea10602954461292184c17759d7bc094698aa7ef38`.
  It does not match the artifact and cannot establish deployed provenance.
- Parent saved actual flash at
  `/home/tane/picomado-backups/20261108-eink1-before-investigation/flash-before.uf2`,
  SHA256 `748845b46da0198b1ec03d44e7a2424315a274b64c45d85e0538f54ca080840c`.
  All 1,533 payload blocks of the existing build match corresponding flash
  addresses, with zero differences. A full-flash backup and program UF2 need
  not have the same whole-file hash.
- Parent's embedded-info read: SDK 2.3.1, build Nov 5, binary end
  `0x1005fcac`. Existing ELF/UF2 timestamps are Nov 5 19:03 JST.
- `docs/changes/EINK-VIDEO-20261105.md:91-95` records writing/verifying exactly
  the `a2f191ee...` artifact on E-ink_1. It also records one initial transient
  TCP error before automatic recovery and a later clean 23-second sample.
  That record is not evidence of historically perfect startup behavior.
- Earlier Nov 1 actual-program backup `c66e58c6...` is a different image;
  `/tmp/picomado-eink1-tMvJKC/E-ink_1.uf2` matches that earlier backup, not the
  currently attached old program.

The Nov 5 source/record is represented by `7c160e3`; its commit time is 19:33,
after the 19:03 build. File history and documented feature state agree, but
there is no embedded source-tree hash and no byte-identical source rebuild was
performed here. Treat `7c160e3..5407f2d` as the supported source comparison,
not a claimed reproducible-build proof. The old compiled program itself is
identified by the parent’s payload comparison.

## Old-image reboot observation

Source: `/tmp/picomado-eink-update-observe-20261108-131505/usb.log` and the
parent's existing server-journal reading for session184. After saving flash,
the parent restarted the unchanged program; no new UF2 was written.

- Frame2440132 USB Receipt end at 11.364403s; Rendered end at 17.998936s.
  Both tcp_write and tcp_output return 0. Device-side interval: 6.634533s.
- Server Receipt at 13:14:51.984; Rendered at 13:15:35.975. Host-side
  interval: 43.991s. Rendered replies for 2440132/2440133 and heartbeats arrived
  together. Subsequent frame2440135 at about 13:16:00 progressed normally.
- Thus the application completed its display/report path well before the
  server obtained the delayed batch. Do not directly subtract host wall time
  and MCU uptime or claim tcp_output return proves RF emission/delivery.
- The USB trace already has `dropped=16`; positive retained events support
  these timings, but the entire startup trace is not lossless. The old image
  lacks the newer ARP/TCP snapshots, so its exact next-hop MAC, lastack and
  retransmission sequence during this event remain unmeasured.

The original firmware-regression hypothesis was tested rather than silently
dropped. Its strong form, that the new firmware is required for delayed/batched
startup replies, is contradicted by this old-image event. A change in frequency
or duration remains untested.

## Compiled and source differences, by layer

New candidate: `/tmp/picomado-eink-audio-E-ink_1/E-ink_1.uf2`, SHA256
`3307b0b52dfd5a6136f5b7d838643dd2f7903e8a51a3026c042f09b420561fe8`.
Binding: `/tmp/picomado-eink-audio-evidence/build-manifest.json`, source
`5407f2dac0c7da37bb68264f80dac4a0eeae2df7`.

1. **Toolchain and common SDK.** Both caches use Release, rp2040/pico_w,
   `/home/tane/dev/PicoW/pico-sdk`, and GCC 13.2.1. SDK generated headers say
   2.3.1. Current clean SDK is `079c6f39023649b154152db30f1d781e884879bc`;
   its reflog has no move after Oct 11. Current submodules: CYW43
   `055d64274b014dd7b1c2fc94d26e8a18face7124`, lwIP
   `77dcd25a72509eb83f72b033d219b1d40cd8eb95`, BTstack
   `eb0bb8b5ea6d234ccb940313b47f7a5c3b4e20ec`.
   Comparing existing shared application-target objects with
   `arm-none-eabi-objdump -dr`, excluding the filename heading, gives 198
   identical instruction/relocation outputs and 6 differing ones. This is not
   a comparison of all debug information, all read-only data, or newly added
   BT objects. The six are main.c, net.c, display_eink.c, cyw43_ctrl.c,
   cyw43_ll.c and cyw43_arch_threadsafe_background.c.
2. **TCP/ARP and bus transport.** In particular tcp.c, tcp_in.c, tcp_out.c,
   etharp.c, cyw43_lwip.c and cyw43_bus_pio_spi.c have matching instructions
   and relocations. `firmware/lwipopts.h` has no source change between
   `7c160e3` and `5407f2d`: MEM_SIZE 16000, TCP_MSS 1460, TCP_SND_BUF 11680,
   queue 32, MEM_LIBC_MALLOC 0 and LWIP_NETIF_TX_SINGLE_PBUF 1 remain.
   The graceful-close/abort-on-close-error policy is unchanged. Its retained
   queues can amplify a stalled connection, but it is not a newly introduced
   difference in this comparison.
3. **Bluetooth changes radio firmware even when no speaker is connected.**
   Old compile flags lack CYW43_ENABLE_BLUETOOTH; new flags set it to 1 and
   link BTstack. `cyw43_configport.h:69-72` selects different embedded WLAN
   images: old `w43439A0_7_95_49_00_combined` is 225240 bytes; new
   `wb43439A0_7_95_49_00_combined` is 232408 bytes, with a separate BT patch.
   The driver downloads the selected WLAN image during chip startup.
   CYW43 low-level initialization also enables F1_INTR and changes the
   interrupt/watermark setup (`cyw43_ll.c:1484,1586,1682`). SDK arch init
   adds `btstack_cyw43_init` on the shared background context. Runtime UI
   Bluetooth OFF is therefore not equivalent to the old firmware.
   However, the existing **compile-out LCD control** already reproduced the
   failure at 10:03 JST: followup.md, “Bluetooth compiled-out control”, UF2
   `85100e8c...`, record `/tmp/picomado-power-observe-20261108-100300`.
   BT is not necessary for those startup failures; this must accompany any
   discussion of the real radio-firmware difference.
4. **Application networking.** New BT builds reduce main JSON storage
   18000→4608 bytes and app_rx 18006→4614 bytes, retain frame-bytes-v1, add BT/
   SBC/media capabilities and messages, and poll BT/audio. Ordinary receipt,
   render and heartbeat writes remain required one-attempt COPY/output paths;
   only optional audio-credit retries use the new bounded backpressure API.
   TCP counters and a read-only ARP/TCP snapshot are added, the latter at
   USB attachment and 15-second USB status cadence. They do not write ARP
   entries. Their timing cost is not instrumented here.
5. **Profiles and display.** New source adds HOME2, explicit AWAY tests and
   menu handling. Comparison of the old/new ELFs' profiles constants finds
   HOME and AWAY fields equal, including credentials, destinations and proxy
   values; no values were output or stored in this evidence. The new third
   profile is additional. Both builds use EEX1 ON and diagnostic options OFF.
   E-ink source changes only status glyph selection for digits; SPI bursts,
   display completion and E-ink pending-frame code do not change. Existing
   trace/USB owner/rx-stats application objects also match.
6. **Memory.** ELF text 398508→553640 bytes, BSS 162360→166608 bytes.
   Static RAM end moves 0x20028ea0→0x2002a370, leaving 94560→89232 bytes to
   heap limit 0x20040000. Main stack reservation remains 4096 bytes. The
   separate bounded lwIP heap remains 16000 bytes, so the BSS change is not
   evidence that lwIP suddenly acquired a smaller allocator. Runtime stack,
   heap high-water marks and radio load were not measured by this comparison.

## Next discriminating comparisons

1. Preserve this exact old-image baseline and its unchanged settings. Prioritize
   identifying the first missing reverse TCP range on the old image, rather
   than treating steady old-device heartbeats as a successful cold-start
   control. A separately authorized passive capture covering both host
   interfaces can compare ARP replies, initial route/MAC, TCP ACK progress and
   retry recovery. Old-image ARP state was not measured in this sample.
2. If firmware incidence remains in question, use the **same E-ink_1** and
   fixed server/AP/profile/settings for repeated, ordered old/new starts,
   measuring Receipt→Rendered and heartbeat arrivals separately. One good
   boot and one bad boot do not establish a regression rate. Restarts/flashes
   remain parent-owned authorized actions; none are initiated by this record.
3. Current-source BT OFF versus BT ON is a useful secondary one-build-option
   control on that same device, but is not a fresh untested idea: LCD already
   failed compiled-out. This switch changes the bundled radio/SDK/app BT
   stack, not one isolated radio instruction. Do not label it a pure WLAN
   blob experiment or expect it to cure the already-reproduced old-image
   fault. Do not mix a Wi-Fi-only blob with active BT as an ad-hoc fix.
4. Preserve server/version/traffic changes as independent candidates. If
   comparing server behavior, vary that layer separately while holding the
   device image and network fixed; current old-image reproduction does not
   replay the Nov 5 server. Do not revert production or change AP/ARP/timeout
   settings merely from this record.

No firmware source, build artifact, private configuration, SDK, device,
service or network setting was changed by this investigator. No SSH was used.
No new build, flash, network probe or test suite was run; offline binary/code
comparison and the parent-owned live observations are the evidence here.
