# Vendored Bluedroid SBC encoder

These 8 C and 4 header files are unmodified from the revision and original paths
in `provenance.json`. Each file retains the Broadcom 1999–2012 Apache-2.0 notice.
The complete license is in `LICENSE-APACHE-2.0`. No NOTICE file is present in the
selected upstream Bluedroid subtree. Preserve these files and notices when
redistributing the service; they are not relicensed under the project's MIT
license. Any later modifications must be identified here and in the manifest.

Only this third-party encoder is included. The separately licensed BTstack core
and `btstack_sbc_bluedroid.c` wrapper (including their noncommercial clause) are
not included or linked into the host service. The firmware's SDK is unchanged.

Build with the ordinary Rust and C toolchain. No Pico SDK or development machine
path, download, helper executable, or dynamic SBC library is required. The
project-owned C ABI is outside this directory. Calls are serialized by the Rust
adapter because the encoder has global joint-stereo scratch arrays.

The source/provenance check is `python3 tests/verify_sbc_vendor.py` from `service`.
