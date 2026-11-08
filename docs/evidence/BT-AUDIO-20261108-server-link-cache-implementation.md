# Remembered server MAC implementation and offline checks

Change ID: [BT-AUDIO-20261107](../changes/BT-AUDIO-20261107.md).
Contract-first commit: `ef62c27`; see the final extension section of
[server-link-hint.md](../design/server-link-hint.md). User approved persistence,
pre-TCP HOME/HOME2 use, finite fallback/update and E-ink_1 testing. This record
covers implementation/host verification, not deployment or effectiveness.

## Product behavior

- Explicit default-OFF `PM_SERVER_LINK_CACHE`, requiring the existing hint feature.
  HOME/HOME2 exact profile/SSID/target key, never AWAY, no credentials persisted.
- Independent sector at physical flash size minus16KiB, eight append slots, each
  explicit little-endian snapshot256B plus commit-marker256B. CRC/version/length/
  sequence/canonical padding checks, same-value dedup, no sequence wrap. Partial
  erase may lose cache and return to dynamic ARP, never reset settings/BT data.
- Main-only storage aggregate and checked20ms SDK flash-safe admission. No core1
  is launched by this firmware. Adapter rejects linked multicore/RTOS use rather
  than assuming an unreviewed core safe. Physical flash operation duration and
  IRQ blackout remain hardware risks, not bounded by the admission timeout.
- Pre-TCP add/readback, provisional ownership across TCP/WS/Hello, same-interface
  number as well as pointer/address/mask/route/generation fencing. First current
  Welcome can replace that owner, update validated RAM and queue persistence.
- Failed cached attempt blocks its original exact seed across re-JOIN/profile
  changes this boot. A second different failed seed conservatively blocks all
  bootstrap for that profile until reboot. Changed current Welcome does not
  relabel the old attempted seed. Existing app_s is the total pre-ACTIVE deadline.
  A normal user close after validated ACTIVE does not block a successful seed;
  closing an unfinished provisional attempt conservatively does. No healthy TTL.
- Old/invalid optional Welcome clears provisional mapping and continues normal
  compatibility; unknown remove failure remains fail-closed. No expired-token
  retries, new ACK-stall reset or unchanged-value flash writes were added.

## Persistence scheduling and observable facts

`event=learn result=changed` means validated RAM changed, not durable success.
Saving runs outside network callbacks/locks from main only after both learning
and a continuous idle interval have reached5s. Busy means WIFI/SERVER/SESSION,
video, E-ink draw/standby/pending/panel work, BT pending/waiting command or media.
If BT is initialized, runtime ready and non-audio are checked under the SDK lock,
then the lock is released before any flash operation. Busy resets the idle timer.
Successful commits and failed attempts are each separated by at least60s.
Changed values coalesce; unresolved safety/IO failure retains dirty RAM.
These checks avoid known startup/negotiation/render work; they cannot prove the
radio will receive no packet during the subsequent interrupt blackout.

Cold marker sequence: `net server_cache event=load result=empty`, normal Welcome
install, `event=learn result=changed`, later `event=save result=ok erase=0|1 us=...`.
Only `save ok` plus readback counts as durable. Warm marker: `load ok`,
`event=installed_before_tcp result=0` and matching IP/MAC before TCP allocation,
then `learn unchanged` with no save. `blocked` followed by `fallback` identifies
finite ordinary-ARP recovery. Log message order is MCU-local; USB delivery is
not TCP delivery or application receipt. No SSID/password is printed.

## Executed host verification

All tests are serial and fake IO. They are not a single RF end-to-end scenario.

- Product codec/aggregate and actual SDK adapter source:12 steps native and
  ASan/UBSan.513 append byte cuts,4609 **one-sector** rollover byte cuts,512-byte
  corruption positions, schema/sequence/key restrictions, failed/uncertain IO,
  no slot reprogramming, dirty-state preservation, neighbor12KiB preservation.
  Command `python3 firmware/tests/server_link_cache_run_tests.py --out DIR`.
  Final logs `/tmp/picomado-server-link-cache-storage-final/`.
- Actual net owner and separate actual main-loop deadline modes:56 checks,
  native/ASan/UBSan. Pre-TCP order; TCP→WS→READY ownership; ten scope changes;
  HOME/HOME2/AWAY; stale/current hints; error/terminal/cleanup; unchanged ACK over
  a simulated1000 hours; app_s3/10/120 exact and+1 boundaries; SESSION never extends
  deadline. Four intentional regressions fail: provisional readiness, missing
  netif-number fence, wrong failure seed, missing app failure notification.
  Command `python3 firmware/tests/run_server_link_cache_owner_tests.py --out DIR`.
  Final logs `/tmp/picomado-server-link-cache-owner-final/`.
- Actual main controller:8 compile/run steps, BT OFF/ON and native/ASan/UBSan.
  JSON-decoded Hello, legacy/invalid Welcome, RAM-before-save, duplicate ignore,
  panel/BT/SESSION idle guards, rate/failure retry and untouched old settings.
  Command `python3 firmware/tests/run_server_link_cache_app_tests.py --out DIR`.
  Final logs `/tmp/picomado-server-link-cache-app-final/`.
- Existing v1 parser/net/main and C/CMake boundaries:30 steps, final logs
  `/tmp/picomado-server-link-cache-v1-final/`. Existing hardcoded static-ARP and
  installed SDK ARP regression:27 steps at `/tmp/picomado-server-link-cache-sdk-final/`.
  Existing BT startup0/1/2 and BT-OFF main regressions pass; log
  `/tmp/picomado-server-link-cache-bt-regression.log`.
- UF2 validator:5 test groups, including every reserved page, bad format/family,
  overlap, duplicate identities and cutoff. Current retained normal and v1 UF2
  both pass final16KiB non-invasion. Legacy56-byte settings HOME2 absent/partial/
  configured fixtures pass. `git diff --check` passes.

Initial app fixture timing was corrected to poll through the new continuous-idle
window; sanitizer retained unused audio handlers, so unreachable audio stubs were
added for linking. Product conditions/assertions were not weakened. The earlier
Python **two-sector proposal** model is separate and was not counted above.
Full root aggregate/service acceptance remains parent-owned. USB, live flash,
router/network settings and SSH were not touched.


## Regular ARM candidate and source freeze

Built after implementation commit `d67ec643a8099da78e62052b4c2cf5ccb0b9d20d`,
with the regular E-ink_1 SDK/config/options and `-j1`. The previous ordinary
build directory and UF2 were preserved. The feature candidate enables CACHE
and HINT, keeps BT on, BT startup diagnostic 0 and fixed-ARP diagnostic off.
All tracked firmware input hashes, private config hash, SDK identity and the
original artifacts were unchanged across the build. Service work is separate.

- Private staging: `/tmp/picomado-server-link-cache-deploy.zjce2qdz/`.
- `cache-E-ink_1.uf2`: 1,126,400 bytes; SHA256
  `2b700eddc6e5dfb56aff0b35d84367226b58408f19257f57be30aa505ceee5c4`.
- ELF text 569,224 bytes, data 0, BSS 167,020; ELF SHA256
  `e945b2b7839697af755c2cab0938650525b393b43d65ddf24459621d0c1e182e`.
- `manifest.json` SHA256
  `bc5c622680b627a81b7aff1ee677840549c4ee64ca9fc27279b1bd36dfda1c8b`.
  This records each input hash, compile definitions, tool arguments and build log.
- Private config SHA256
  `087802528f5037dd2d5301a298e714b79a2936d7a1a337a0817297dbef7ee34f`.
  No private config values are included here.
- SDK commit `079c6f39023649b154152db30f1d781e884879bc`; submodule identities
  are in the manifest. There are no linked core1-launch or RTOS-task symbols.

The generated linker region ends at `0x101fc000` (physical 2 MiB minus 16 KiB).
An actual ARM linker fixture accepts a small section and rejects an overflowing
section; the replacement candidate below additionally tests the full span. An independent compile assertion confirms the physical
flash-size macro remains 2,097,152. Product UF2 verification passes all 2,200
blocks: payload end `0x10089800`, below cache start `0x101fc000`. Cache, BT and
legacy settings therefore are outside the payload. This verifies file/linker
boundaries, not a live flash tool's behavior; parent acceptance must compare
pre/post device readback, especially the unchanged final 12 KiB.

Compile commands confirm the static-entry definition reaches SDK `etharp.c`.
Private feature-OFF recompiles of main/net match the prior ordinary objects'
ARM instructions and relocations. Original normal UF2 remains SHA256
`ecaad23d53a85ef64b042138ed27c2f7655402c231efaf5d8ff8f17cc4d8516a`.

`postbuild-checks.txt` records actual ELF placement: flash callback
`execute_flash_operation` is SRAM `0x200000e0`, SDK erase/program are SRAM
`0x200004c4`/`0x20000550`. The compiler estimates main 896 bytes, flush 1,032
and scan 1,304, or 3,232 bytes for that nested path before leaf/IRQ usage in the
4,096-byte core0 stack. This is not a complete runtime stack high-water proof.
Cold persistence, warm pre-TCP use, radio behavior during flash blackout, and
recovery effectiveness still require the separately authorized device trial.


## Pre-deployment correction: bounded stack scratch

The first `2b700edd...` candidate above was **withheld before device deployment**
after parent review of the 3,232-byte partial stack estimate. Specification
`b3a47df` requires aggregate-owned static scratch and defensive recursion
rejection, without changing the format or persistence/fallback behavior. The
replacement's implementation/tests/artifact are recorded below only when run.

The SDK initialization concern was checked using the actual installed
`pico_flash/flash.c` and `pico/flash.h`, not only adapter mocks. Native and
ASan/UBSan fixtures execute before explicit core-init at timeout 0, 20 and
UINT32_MAX, after init/deinit, and with IRQs initially disabled. All pass with
exactly one callback and IRQ-state restoration per call. Effective ARM compile
flags were also used for preprocessing: multicore/FreeRTOS support are absent,
ASSUME_CORE0/1 are zero, core-init returns true, enter only disables local IRQs
and returns OK, exit restores them and returns OK. No lockout or timeout branch
remains, so this single-core build does not require initializing a nonexistent
lockout victim before its first save. No safety override was introduced.

Command: `python3 firmware/tests/server_link_cache_flash_safety_test.py --sdk
/home/tane/dev/PicoW/pico-sdk --out /tmp/picomado-server-link-cache-flash-safety
--compile-commands /tmp/picomado-server-link-cache-deploy.zjce2qdz/build-cache/compile_commands.json`.
Six compile/run/preprocessor steps pass; SDK hashes and logs are in that output.
The first fixture parser expected `bool` in preprocessed text; it was corrected
to match the function name because the actual C preprocessor emits `_Bool`.
Neither the SDK nor its branching conditions were changed. This is no guarantee
of flash hardware success, blackout duration or asynchronous radio behavior.


The scratch correction's product storage suite passes all 12 native/sanitizer
steps, including the original byte-cut/corruption/neighbor-preservation cases
and new callback re-entry during load, needs-erase, append, rollover and failures.
Re-entry leaves pending HOME/HOME2 values, IO bindings, output buffers and durable
bytes unchanged, and failure exits release the guard. Logs:
`/tmp/server-link-cache-owned-scratch-tests/`. Static scratch occupies 2,152 bytes
plus the independent one-byte guard; no heap, format or scheduling change.

After this correction, actual owner/main-deadline 56 checks, actual main BT OFF/ON
8 steps and prior v1 30 steps all pass again with native/sanitizer coverage:
`/tmp/picomado-server-link-cache-scratch-owner/`,
`/tmp/picomado-server-link-cache-scratch-app/`, and
`/tmp/picomado-server-link-cache-scratch-v1/`. An initially unsupported `--sdk`
argument to the v1 runner was removed and the full runner then passed.
Same-flags ARM translation-unit preview reduces scan to 56, flush to 56 and
encode to 48 bytes; regular source-fixed artifact checks follow separately.


### Replacement candidate after scratch correction

Implementation commit `ad20ad321c0fcf524d78a97e64e79c68a83baf7d` is frozen for
this regular `-j1` build. It supersedes the withheld `2b700edd...` candidate.

- Private staging: `/tmp/picomado-server-link-cache-scratch-deploy.yzn_e9v3/`.
- `cache-E-ink_1.uf2`: 1,126,400 bytes, SHA256
  `a30bd867bd3a4b8a8690b9be402e516700b1f45668f372ac3601c60815883e31`.
- `manifest.json` SHA256
  `a843029da705764a8fb74e5cb467d03bf2f49ccea322954f06a4d9e0ed298e65`.
- ELF SHA256 `f4386a902696d45406d5b83d6cfb2b3e01128ccab8e411e303bd42822970dd21`;
  text 569,320, data 0, BSS 169,148 bytes. Actual linked BSS is 2,128 bytes more
  than the withheld candidate; the compiler/linker layout is authoritative.

Actual ARM frames now are main 896, flush 56, scan 56 and encode 48 bytes: the
identified nested codec path is 1,056 bytes instead of 3,232. This leaves much
more of the 4,096-byte core0 stack for other calls/interrupts, but is still not
an IRQ-inclusive runtime high-water proof. `stack-usage.json` records the frames.
Flash callback/erase/program remain in SRAM and the final ELF uses the SDK's
weak default flash-safety helper, with no core1 launch/RTOS symbols. The actual
compile-flags preprocessing and installed SDK native/sanitized tests all pass
again under `sdk-flash-safety/`; their summary hash is in the manifest.

Source/config/SDK/original-artifact hashes are unchanged across the build, and
feature-OFF main/net instructions and relocations still equal the original
normal build. The UF2's 2,200 blocks end at `0x10089800`, below reserved
`0x101fc000`. `linker-boundary/fit-full*` additionally verifies a 2,080,768-byte
section fits exactly to the permitted FLASH end; one byte more is rejected.
Physical flash remains 2 MiB. Private config and SDK identities remain those
listed above. Original normal UF2 and the previous candidate are retained.

The initial metadata writer reused its log-path variable as an open stream after
ARM compilation/linking completed. Metadata-only resumption corrected this;
it did not repeat the ARM build or alter product inputs. Both driver/resume logs
are retained in `/tmp/picomado-server-link-cache-scratch-build-driver.log` and
`/tmp/picomado-server-link-cache-scratch-resume.log` respectively. No hardware
was touched. Parent owns selection, cold save/readback, warm pre-TCP use, stored
BT/settings preservation, radio acceptance and rollback decisions.
