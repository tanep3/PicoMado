#!/usr/bin/env bash
# Offline standalone codec tests: no service Cargo build, SDK or device access.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d /tmp/picomado-sbc-tests.XXXXXX)
trap 'rm -rf -- "$work"' EXIT
python3 "$root/tests/verify_sbc_vendor.py"
for source in "$root"/vendor/bluedroid-sbc/srce/*.c "$root/native/sbc_encoder_abi.c"; do
    "${CC:-cc}" -O2 -std=c11 -fno-strict-aliasing -fwrapv -I"$root/vendor/bluedroid-sbc/include" \
        -c "$source" -o "$work/$(basename "${source%.c}").o"
done
"${AR:-ar}" crs "$work/libpicomado_sbc_encoder.a" "$work"/*.o
"${CC:-cc}" -std=c11 -Wall -Wextra -Werror "$root/tests/sbc_encoder_abi_test.c" \
    "$work/libpicomado_sbc_encoder.a" -o "$work/abi-tests"
"$work/abi-tests"
"${RUSTC:-rustc}" --edition=2021 --test "$root/src/sbc_encoder.rs" \
    -L "$work" -l static=picomado_sbc_encoder -o "$work/sbc-tests"
"$work/sbc-tests" --test-threads=4
