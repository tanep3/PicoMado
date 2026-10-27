# PUBLIC-CUSTOM-HARDWARE-20261027 — custom build contract

## Decision and comparison

The prior public portability contract rejected every profile not on a three-name hardware whitelist. This blocked a Pico W with the same implemented display and input path but different valid wiring. The user has withdrawn the verified-only build rule. The existing LCD and E-ink profile layouts remain positive controls; an untested button/wiring layout is accepted for compilation if structurally valid. A passing build does not grant physical verification status.

## Contract

A device selects a single named profile from `firmware/profiles/` with a `.h` and `.board` file. The name is a safe single path component. Its board must be `pico_w`, the MCU implementation currently provided. The profile must select the implemented ST7735S 160×80 RGB565 LCD with four buttons, or V4 250×122 mono1 E-ink with zero or two buttons. GPIO values must be Pico W pins, unique among active display and input signals, and SPI SCK/MOSI must belong to the selected Pico SPI controller. Inconsistent configuration, missing implementation, and changed artifacts remain rejected. The per-device config cannot override the profile's hardware macros or include conditional hardware definitions. Build lineage and offline preflight continue binding the selected profile, board, configuration and UF2 by digest. Verification status is a public support fact, not a build admission predicate.

## Evidence and limits

The two-button V4 E-ink device was demonstrated physically by the user. Public support tables say 動作確認済み / operation checked. Older wording incorrectly generalized incomplete acceptance of specific later checks to an untested complete device. This reported demonstration does not validate the firmware built from this change. Compare LCD, E-ink, new valid wiring, conflicts and unavailable implementations offline; physical behavior of the new build remains untested. The root gate's machine-local evidence paths are a separate portability blocker requiring a preserved-provenance redesign; this change does not declare operational completion.
