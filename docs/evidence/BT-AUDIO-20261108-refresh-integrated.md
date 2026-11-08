# Bluetooth refresh/reason diagnostics: integrated candidate

Fixed source `502b2cbfdf0c49682bf3a2e1244d079044b90f83`, clean firmware tree before and after testing/building. The parent reports the previous 582271 image was already running with HOME2 selected; this worker did not access the device or independently verify that deployment.

## Preserved prior image

Before overwriting incremental outputs, the exact previous UF2, `candidate-manifest.json` and `implementation-checks.json` were copied to `/tmp/picomado-candidate-backup-582271-20261108/` and verified byte-for-byte/SHA256. The retained UF2 SHA is `582271d1f83bbf7623187cd553a88736cc6406264261acc28599517c16c7e497`. No private config was copied.

## New candidate

- UF2: `/tmp/picomado-home2-integrated-lcd/PicoMado-lcd1.uf2`.
- SHA256: `0cf3f44299548597e10f0c272260fc015f23f9428b7d57816cba4a55061e56ee`.
- New candidate manifest and implementation checks are in that same build directory. The candidate manifest remains not-deployed; physical application is separate parent evidence.
- UF2: 2171 validated blocks, range `[0x10000000, 0x10087b00)`. Bluetooth storage at `0x101fd000` and settings at `0x101ff000` remain excluded.
- ELF: text561832, data0, BSS238236 bytes. Versus 582271: text +768, BSS +8. Static linker heap gap17588 bytes; no runtime high-water claim.
- All PM cache flags, private config SHA and pinned SDK LL SHA are unchanged from 582271. BT/EEX1/credit/sleep/ARP_RX ON, HCI0, SPI_REAL_SIZE/INNER OFF, other probes OFF. No secret values were printed or committed.

## Checks

On frozen source, `firmware/tests/run_host_tests.sh`, `run_contract_tests.sh` and `run_usb_tx_tests.sh` all passed in sequence. This includes new same-peer refresh ownership/deadline checks and existing HOME2/settings/USB/EEX1 regressions. Combined log: `/tmp/picomado-bt-refresh-integrated-tests.log`.

Incremental LCD cross-build passed without compiler warning/error lines. Log: `/tmp/picomado-bt-refresh-integrated-build.log`. The shared SDK was not edited, and source/config/SDK/flags/UF2 bounds were rechecked.

The optional root gate was rerun. It remains rejected on the same one pre-existing route-fixture mismatch: `test_gate_routes.Routes.test_actual_firmware_build_sdk_flags_and_offline_flash`, missing approval fixture expected to fail but current route succeeds. 49 tests ran, one failed; public export and hardware profile checks passed. Log: `/tmp/picomado-bt-refresh-integrated-gate/structural.log`. Do not call this a complete root gate pass.

No USB, flash, service, SSH, push or network-setting operation was performed by this worker. The new candidate's physical same-peer refresh, terminal-reason output and end-to-end effects remain pending parent application and independent verification. The saved HOME2 selection is not changed by this image's payload.

The original 0cf3 UF2 and manifest/checks were subsequently retained, byte/SHA-verified, at `/tmp/picomado-candidate-backup-0cf3-20261108/` before the [font-corrected candidate](HOME2-PROFILE-20261108-font.md) replaced the working build outputs.
