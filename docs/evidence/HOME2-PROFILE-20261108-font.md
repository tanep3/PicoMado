# HOME2 missing digit: font correction and pixel evidence

## Cause and missed test boundary

The user reported HOME2 appearing as HOME plus a blank cell, then specifically suggested the digit2/font. The production LCD and E-ink local fonts contained A–Z only; both renderers skipped every other character while advancing a 12-pixel cell. `strlen("HOME2")` still centered five cells. HOME2 fits both length limits (LCD13, E-ink20), so width, buffer truncation and reversed string order do not explain the missing digit.

The earlier HOME2 application/USB tests used display mocks and verified the string `HOME2`, not its resulting pixels. That test boundary missed a real user-visible defect. The corrective tests now run the production renderers and inspect the output pixels, in addition to retaining the logical tests. This is a concrete coverage correction, not a claim that all visual defects are eliminated.

## Implementation

Specification `3d710c1`; implementation `af0db01`. A shared compact `status_font.h` preserves every legacy A–Z bitmap byte and adds digits0–9. The LCD and E-ink renderers only replace their glyph lookup. Placement, 2x scale, 12-pixel cells, centering, clipping, color, rotation, panel compensation, network profiles and persistent settings remain unchanged.

## Pixel and visual checks

- All26 existing uppercase glyphs compare byte-for-byte with the legacy font. All256 byte inputs are checked for valid/unsupported lookup behavior; digits are nonempty/distinct and digit2 is checked against an independently written seven-row shape.
- Actual LCD SPI pixels are captured and decoded through the inverse of the existing rotation/color transform. Actual E-ink V4 RAM bytes are captured and decoded using the established landscape mapping.
- Both actual outputs match independent whole-frame expectations for NETWORK with HOME, HOME2, AWAY, digit2 alone, unsupported characters, empty text and overlength clipping.
- The old production display sources (unchanged between 28afc95 and 502b2cb) reproduce the missing digit in the same output capture. Before/after images were visually inspected: [LCD](HOME2-PROFILE-20261108-lcd-font.png), [E-ink](HOME2-PROFILE-20261108-eink-font.png). These are captured software-rendered pixels, not photographs of a physical panel.
- Native and ASan/UBSan focused tests passed. Initial LeakSanitizer startup failed because of the execution environment's ptrace restriction; rerun used `ASAN_OPTIONS=detect_leaks=0`. No leak-check pass is claimed. Log: `/tmp/picomado-home2-font-asan.log`.

The focused checks are reproducible with `bash firmware/tests/run_status_font_tests.sh <output-directory>` and are included in the full host suite.

## Frozen candidate

Source `29b300092f55f1438d645819fd0d8e63b1224f20` includes the font implementation plus the parent's documentation commit. Full host, contract and USB TX suites all passed on that fixed firmware tree. Combined log: `/tmp/picomado-home2-font-integrated-tests.log`. LCD cross-build passed: `/tmp/picomado-home2-font-integrated-build.log`.

- UF2: `/tmp/picomado-home2-integrated-lcd/PicoMado-lcd1.uf2`.
- SHA256: `80367df65f805f004ff5ab74403acdf2025e1d83198d2cf1d09e72d7826d2d50`.
- Candidate manifest and implementation checks are in the same directory; the manifest records not deployed. Parent physical application is separate evidence.
- 2174 validated UF2 blocks, range `[0x10000000, 0x10087e00)`. Bluetooth/settings storage remains excluded.
- ELF text562656, data0, BSS238236 bytes; static heap gap17588 bytes. BSS/heap gap unchanged from 0cf3. No runtime high-water claim.
- Private config SHA, all PM flags and pinned SDK LL SHA remain identical to 0cf3. No private config values were printed or committed, SDK originals were not edited, and saved profile selection is not changed by the firmware payload.
- Old0cf3 UF2 and manifests were preserved and byte/SHA-verified at `/tmp/picomado-candidate-backup-0cf3-20261108/` before rebuilding.

Optional root gate was rerun: 49 tests, the same one pre-existing route-fixture mismatch failed (`test_gate_routes.Routes.test_actual_firmware_build_sdk_flags_and_offline_flash`). No root gate pass is claimed. Log: `/tmp/picomado-home2-font-integrated-gate/structural.log`.

No USB, flash, service, SSH, push or network-setting action was taken by this worker. The parent still needs to confirm the physical NETWORK/HOME2 screen visibly ends in2 after authorized application. These are implementation checks, not independent acceptance.

## LCD application record

The parent worker applied this candidate to LCD serial E661640843239D21 at11:22 JST after checking music was stopped. `/tmp/picomado-home2-font-flash.log` reports flash verification OK for SHA25680367df65f805f004ff5ab74403acdf2025e1d83198d2cf1d09e72d7826d2d50. Selected AWAY and user volume36 were preserved; no music or input command was sent. Passive boot record `/tmp/picomado-power-observe-20261108-112240` shows Wi-Fi retry followed by IP assignment, then EEX1 endpoint discovery with22 sends/0 receives and no candidate. This is an outstanding connection-preparation condition, not proof of a font failure or of network reconnection. Physical sight confirmation of HOME2 remains with the user; rendering-buffer images and tests are not a camera observation.
