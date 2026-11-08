# HOME2 implementation checks (2026-11-08)

Specification: `4ec8f52`, based on `a9b361f`. Stable IDs remain HOME=0, AWAY=1; HOME2=2 is a new optional direct profile. The ignored LCD config moved its existing commented second home credentials into HOME2 without changing the active HOME or any AWAY definition. Credentials are not in this evidence or Git.

Passed implementation checks:

- Full `firmware/tests/run_host_tests.sh`, explicitly using `/home/tane/dev/PicoW/pico-sdk`. Log: `/tmp/picomado-home2-host.log`.
- New settings fixtures: absent, partial and configured HOME2; full valid records for old IDs 0/1; profile2 persistence/fallback; unchanged 56-byte layout/CRC offset52; invalid ID/CRC; all navigation masks and both directions.
- Standard USB INPUT against actual production main/parser: configured HOME2 on LCD and two-button E-ink; unset HOME2 on all existing sample variants; connection/WIFI/SERVER/SESSION/RETRY/ENDPOINT entry, hidden unset choice, forward/reverse/wrap, duplicate IDs, save and cancel, other settings retained.
- EEX1 production-main test: HOME2 bypasses discovery, polling, endpoint reporting and DHCP-driven EEX side effects; existing HOME/AWAY EEX tests retained.
- `firmware/tests/run_contract_tests.sh`: passed; log `/tmp/picomado-home2-contract.log`.
- `firmware/tests/run_usb_tx_tests.sh`: passed; log `/tmp/picomado-home2-usb.log`. Fixtures only, no real USB access.
- `git diff --check`: passed for HOME2 files.

The first host attempt used the inherited `PICO_SDK_PATH=/home/tane/pico/pico-sdk`, which failed the pinned source hash check. No SDK source was modified. Rerunning with the requested SDK above passed. The initial optional root gate attempt refused to overwrite an immutable receipt; a new receipt path was used for the next attempt.

These are worker implementation checks, not independent acceptance. Other Bluetooth workers began changes during the regression run. A final combined source-frozen regression and LCD build will be reported separately. No device flashing, service change, network configuration, SSH or push was performed. Physical HOME2 connectivity and end-to-end effectiveness remain unverified.

Downgrade: old firmware rejects persisted profile2 and discards other settings. Before installing it, confirm HOME or AWAY in this firmware's NETWORK menu and verify the selection. This writes an old-readable 0/1 record preserving other fields. Selecting only a candidate or cancelling does not make the saved profile safe for downgrade. See the specification for recovery limitations.
