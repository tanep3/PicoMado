#!/usr/bin/env python3
"""Offline production configuration -> installed lwIP output comparison.

Only local compiler/child test processes and explicit output files are used.
No socket, USB, deployment, capture, SDK edit, or download is performed.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import resource
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sdk", type=Path, required=True)
    parser.add_argument("--baseline-net", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    firmware = Path(__file__).resolve().parents[1]
    lwip = args.sdk.resolve() / "lib/lwip/src"
    baseline = args.baseline_net.resolve()
    results = []
    sdk_files = [lwip / "core/tcp_out.c", lwip / "core/def.c", lwip / "core/stats.c", lwip / "include/lwip/tcp.h"]
    hashes = {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in sdk_files}
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))

    def run(label, command, expected_failure=False):
        env = dict(os.environ, ASAN_OPTIONS="detect_leaks=0")
        process = subprocess.run(list(map(str, command)), cwd=firmware, env=env,
                                 capture_output=True, text=True, timeout=60)
        passed = process.returncode != 0 if expected_failure else process.returncode == 0
        if expected_failure:
            passed = passed and "flags_at_connect&TF_NODELAY" in process.stderr
        log = args.out / (label + ".log")
        log.write_text(process.stdout + process.stderr)
        results.append({"label": label, "exit": process.returncode,
                        "expected_failure": expected_failure, "passed": passed,
                        "command": list(map(str, command)), "log": str(log)})
        (args.out / "results.json").write_text(json.dumps(results, indent=2) + "\n")
        if not passed:
            raise RuntimeError(label + ": " + (process.stdout + process.stderr)[-2000:])

    cc = ["cc", "-std=gnu11", "-O1", "-g", "-Wall", "-Wextra", "-Werror"]
    for variant, sanitizers in [("native", []), ("sanitized", ["-fsanitize=address,undefined", "-fno-omit-frame-pointer"])]:
        flags = {}
        for source, before in [("before", True), ("after", False)]:
            for device in ["sample-a", "E-ink_0", "E-ink_1"]:
                label = f"app-{variant}-{source}-{device}"
                binary = args.out / label
                extra = [f'-DPM_NET_SOURCE="{baseline}"', "-DPM_EXPECT_LCD_NODELAY=0"] if before else []
                command = cc + sanitizers + ["-DPM_USE_SAMPLE_CONFIG=1", "-Idevices/" + device,
                    "-Isrc", "-Itests/net_stubs", "-Itests/app_stubs", "-I."] + extra + [
                    "tests/net_nagle_test.c", "src/ws_accept.c", "-o", binary]
                run("compile-" + label, command)
                flags_path = args.out / (label + ".flags")
                run(label, [binary, flags_path])
                flags[source, device] = flags_path

        # The new application assertion must fail on the unfixed LCD source.
        label = "regression-before-" + variant
        binary = args.out / label
        run("compile-" + label, cc + sanitizers + ["-DPM_USE_SAMPLE_CONFIG=1",
            "-Idevices/sample-a", "-Isrc", "-Itests/net_stubs", "-Itests/app_stubs", "-I.",
            f'-DPM_NET_SOURCE="{baseline}"', "tests/net_nagle_test.c", "src/ws_accept.c", "-o", binary])
        run(label, [binary, args.out / (label + ".flags")], expected_failure=True)

        # Compile the actual installed tcp_out.c, not a copied Nagle model.
        label = "sdk-output-" + variant
        binary = args.out / label
        run("compile-" + label, cc + sanitizers + ["-ffunction-sections", "-fdata-sections",
            "-Itests/nagle_sdk_stubs", "-Itests/app_stubs", "-I.", "-I" + str(lwip / "include"),
            "tests/net_nagle_sdk_test.c", lwip / "core/tcp_out.c", lwip / "core/def.c",
            lwip / "core/stats.c", "-Wl,--gc-sections", "-o", binary])
        for source in ["before", "after"]:
            for device in ["sample-a", "E-ink_0", "E-ink_1"]:
                expected_output = int(source == "after" and device == "sample-a")
                run(f"sdk-{variant}-{source}-{device}",
                    [binary, flags[source, device], expected_output])

    assert all(hashlib.sha256(Path(p).read_bytes()).hexdigest() == value for p, value in hashes.items())
    summary = {"all_pass": True, "steps": len(results), "SDK_source_unchanged": True,
               "SDK_hashes": hashes, "real_TCP_output": True,
               "transport_and_allocation": "deterministic mocks; no real timers, radio, USB, or network",
               "sanitizers": "ASan+UBSan; LeakSanitizer disabled in ptrace environment"}
    (args.out / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
