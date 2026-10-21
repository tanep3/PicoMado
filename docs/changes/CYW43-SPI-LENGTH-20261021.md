# CYW43 SPI command length: bounded LCD comparison candidate

Status: design before implementation or firmware installation. Scope is PicoMado-lcd1 only. The currently running LCD UF2 is SHA256 `efa09ede35dddd08d1b69ac988d0bada646e1605d1307e3bfdb5e37e8068233a`; the retained recovery UF2 is SHA256 `9d4f66d1b71e07ef5c6e85b802201d0b6712b6290cd1c9133b40bdbf36479687`. Neither board nor the shared SDK is changed by this design.

## Contract and evidence

The pinned `cyw43-driver` gitlink is `055d64274b014dd7b1c2fc94d26e8a18face7124`. In its `cyw43_sdpcm_send_common`, the SDPCM header records the real `size`, then passes `CYW43_WRITE_BYTES_PAD(size)` to `cyw43_write_bytes`. The Pico SDK PIO SPI implementation uses that argument both for the F2 SPI command length and, after 4-byte alignment, for transfer length. Upstream [PR 155](https://github.com/georgerobotics/cyw43-driver/pull/155), commit `97077de327c67bf78603b17db9fa3e2f6ca24cd3`, changes the SPI branch to pass `size`; it leaves the SDIO branch padded.

For the saved 99-byte TCP packet, Ethernet is 153 bytes; SDPCM is 12 + 2 + 4 + 153 = 171 bytes. The current F2 command length is 172 and the corrected command length is 171; the physical SPI transfer remains 172 payload bytes. The existing SDPCM header remains 171. Saved trials include both received and missing first 99-byte packets with the same build, so padding is a real code defect but **does not distinguish those outcomes**. A zero bus return is PIO transfer completion, not chip acceptance or RF delivery. The server's ACK-independent latest-frame delivery remains active and separate.

## Smallest implementation and offline proof

Reuse the existing pinned private `cyw43_ll.c` generator. Add one explicit LCD-only build option that requires the existing inner probe configuration and replaces only the final `cyw43_write_bytes(... CYW43_WRITE_BYTES_PAD(size), ...)` argument with `size` in the generated unit. Do not edit SDK files, create a second send path, change diagnostics, add I/O to the hot path, or change network settings. The default option must generate byte-for-byte the existing private unit.

Keep the current LCD build flags, board ID, config, sources, and linked SDK fixed. Save the current UF2/ELF and hashes before building. Local proof must compare baseline and candidate generated sources, assert exactly the SPI command-length expression changed, exercise the real SDK send functions with 153-byte Ethernet and a nonaligned short control case, and show the same SDPCM header, bytes through its real length, aligned physical transfer size, result and callback behavior. Build the LCD target and retain UF2/ELF/object hashes; a different binary SHA is expected, while target scope and settings-sector exclusion must be verified. No board writing is authorized in this step.

## Later finite hardware comparison

After explicit LCD BOOTSEL and recovery preparation on a later turn, compare the current and corrected LCD firmware under the same server, connection conditions, buttons, selection and observation limits. Check matched first-send 99-byte heartbeat/button segments, Pi arrival and ACK, retransmissions, frame draw time and diagnostic loss. Use a finite set of operations and preserve rain-radar at the end. If the corrected build still loses packets, the padding fix was insufficient; if the loss rate improves, repeat a bounded control before assigning causality. The known before/after events are separate connections and cannot alone establish a paired causal difference. Keep the current firmware installed tonight; E-ink firmware work is separate.
