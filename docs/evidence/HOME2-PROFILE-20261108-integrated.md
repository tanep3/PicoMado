# Frozen integrated candidate: HOME2 and Bluetooth fixes

## Source and build

- Fixed source: `5fceee8cb783f5b06c5de233d9b0bb06625aab2b`, clean firmware tree before and after building/testing.
- Contains HOME2 specification `4ec8f52`, implementation `ee29031`, flat sample correction `5d0a8b0`, stable observation action IDs `5fceee8`, Bluetooth configuration watchdog `26953a6`, send-backpressure handling `100bcf6`, and portable SDK fixture `4059d03`.
- New build directory: `/tmp/picomado-home2-integrated-lcd`. Existing ARP diagnostic builds were not overwritten.
- UF2: `PicoMado-lcd1.uf2`, SHA256 `582271d1f83bbf7623187cd553a88736cc6406264261acc28599517c16c7e497`.
- `candidate-manifest.json` and `implementation-checks.json` in the build directory bind source/config/SDK/artifact and test results. Private config contents are never included. The candidate manifest records **not deployed**; any later physical application belongs to the parent's separate evidence.

All PM cache flags match `/tmp/picomado-arp-rx-lcd/CMakeCache.txt`: BT ON, HCI drain limit0, EEX1 ON/48116, SPI_REAL_SIZE OFF, INNER OFF, credit/sleep/ARP_RX ON, AP2/event/stream/TX/button simulation OFF. Main's actual compile command was checked too. Private HOME=L13 and HOME2=Aterm definitions are complete and distinct; the existing commented Aterm credential pair was moved without changing HOME or AWAY definitions. HOME2's server aliases the existing HOME direct server. Network reachability is not proven by those config checks.

The pinned SDK at `/home/tane/dev/PicoW/pico-sdk` remains unchanged by this task. The checked `cyw43_ll.c` SHA is `7f4c93755b2ab911d18606652497b2f3cd08085ce63b3b1836171d375f806b10`. Private generated diagnostics remain build-local.

## Implementation checks

With source frozen, `run_host_tests.sh`, `run_contract_tests.sh`, and `run_usb_tx_tests.sh` all exited successfully in sequence. Combined log: `/tmp/picomado-home2-integrated-tests.log`. LCD cross-build exited successfully; log `/tmp/picomado-home2-integrated-build.log`. No compiler warning/error lines were found in that build log. HOME2-specific absent/partial/configured, legacy settings, USB menu and EEX-bypass assertions are described in the [focused record](HOME2-PROFILE-20261108-host.md).

Optional root gate is **not passed**. Its 49 structural tests have one remaining failure: `test_gate_routes.Routes.test_actual_firmware_build_sdk_flags_and_offline_flash` expects a missing approval fixture to reject, but the existing current route returns success. This HOME2 work did not change that route or its test. Public export and hardware validation now pass. Logs: `/tmp/picomado-home2-integrated-gate/structural.log`.

## Artifact bounds and memory

UF2 validation checked all 2168 block headers, payload lengths, indices/counts, unique address ranges and reserved-region exclusion. Its payload range is `[0x10000000, 0x10087800)`, below Bluetooth flash storage at `0x101fd000` and settings at `0x101ff000`. No reserved settings bytes are in this image.

ELF size: text561064, data0, BSS238228 bytes. Compared with the ARP_RX baseline: text +2128 bytes, BSS +24 bytes. Linker static heap gap is 17596 bytes; this is not a runtime stack/heap high-water measurement or a guarantee against all allocations. Existing stack configuration remains unchanged.

## Remaining acceptance

This is implementation evidence only, not independent review. No USB, flash, service, SSH or network-setting operation was performed by this worker. HOME2 physical connection, menu selection/reboot retention on the device, Bluetooth watchdog/backpressure effects and independent acceptance remain for the parent. The known old-firmware profile2 incompatibility remains: select and confirm HOME or AWAY using the new firmware before downgrading, or the old firmware can discard the complete settings record. See the [contract](../changes/HOME2-PROFILE-20261108.md).

## Artifact retention after the next candidate

Before the incremental 502b2cb build replaced the working output, the exact 582271 UF2 and its original candidate manifest/checks were copied and SHA-verified in `/tmp/picomado-candidate-backup-582271-20261108/`. The parent records physical application separately. The newer build is documented in [the refresh integration record](BT-AUDIO-20261108-refresh-integrated.md).
