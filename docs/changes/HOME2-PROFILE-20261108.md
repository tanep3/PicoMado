# HOME2-PROFILE-20261108: optional second home network

## Authority and scope

Extend the canonical kernel connection contract and EEX1-RESTORE-20261022. The user requests HOME for L13, HOME2 for Aterm and AWAY for tethering. Existing HOME/AWAY are the working reference, not a new network stack. Keep the radio diagnostics, Bluetooth build, settings sector and other settings unchanged. No device, USB, service, SDK-original, SSH, network configuration or push operations are part of this change.

## Model and transition contract (before implementation)

- Stable persisted identities: HOME=0, AWAY=1, HOME2=2. Never renumber AWAY. Display/navigation order is HOME → HOME2 → AWAY, cyclic; DOWN advances and UP reverses.
- HOME and HOME2 are direct TCP profiles. Only explicit AWAY identity enters EEX1 discovery, endpoint progress/reporting or SOCKS5. Existing AWAY credentials and EEX1 behavior are unchanged.
- Optional HOME2 SSID/password/server address default to empty when absent in old configs. Empty/incomplete optional profiles are skipped in menu traversal. Traversal is bounded; if there is no configured alternative, leave the selection unchanged.
- SUB during connection/retry opens NETWORK and cancels the in-flight connection. UP/DOWN only change the candidate. SUB saves the candidate and starts connection. POWER cancels without a flash write and resumes the original selection. Normal ACTIVE SUB remains the plugin action, not a new settings entrance. GPIO and standard USB INPUT use the same transitions.
- Keep the existing settings layout, magic, CRC and field bounds. Valid old profile 0/1 records load without changing incarnation, revision, timeouts or input threshold. Accept profile 2. If a saved HOME2 is unconfigured, normalize only profile to HOME in RAM; keep every other valid field and do not write merely on boot. Unknown IDs/corrupt CRC retain existing invalid-record behavior.
- Local ignored LCD config uses the existing L13 HOME values and moves the commented Aterm SSID/password pair to HOME2. HOME2 uses the same direct server as HOME. Keep AWAY byte-for-byte unchanged. Never print or commit credentials. Public samples contain only empty/dummy values.

## Downgrade safety

Old firmware rejects profile 2 and discards the entire settings record, including other settings. This implementation does not pretend that preserving IDs alone provides backward readability. Before downgrading to firmware without HOME2, use this firmware's NETWORK menu to select and confirm HOME or AWAY (0/1), then verify the selected profile before flashing the old image. This save preserves all other current fields and writes the same CRC/layout. If the old image has already booted with profile 2, it may have overwritten settings with defaults; do not claim recovery without a pre-existing settings backup. Do not modify settings flash externally as part of this task.

## Acceptance and evidence boundaries

Host tests must cover legacy records 0/1, profile 2 round-trip, missing HOME2 fallback preserving other settings, invalid IDs/CRC, navigation and reverse/wrap/skip, repeated input, confirm/cancel, both home profiles bypassing EEX1, and standard USB INPUT semantics. Run existing host and USB/contract suites and an isolated LCD cross-build with BT ON, HCI drain 0, SPI_REAL_SIZE OFF, INNER OFF and the a9b361f ARP/credit/sleep diagnostic flags retained. Record UF2 source and SHA, settings-sector bounds and build results. Local tests are implementation evidence only; independent verification, physical application and connectivity/effectiveness remain separate and pending.
