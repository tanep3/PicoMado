# Development process — OBS-20261021-01

The entry is [AGENTS](../AGENTS.md); generic principles are [standard v1](../development/standard/v1/standard.md). The existing [contracts and observation design](contracts-observation-design.md#obs-20261021-01-observation-domain) owns the project domain. The sole change state is [OBS-20261021-01](changes/OBS-20261021-01.json). This is development-process prevention, separate from the latency cause investigation.

## Existing mechanisms and gap

At base `86ca475`, tracked root has no AGENTS, unified root check or CI workflow. Existing mechanisms are contract/verification pairs, `docs/checkpoints/*`, fixed commits, independent reviews, firmware `run_host_tests.sh`/`run_usb_tx_tests.sh`, Cargo tests and per-trial verification scripts. `firmware/tools/build.sh` creates a UF2 hash manifest. `flash.sh --check-only` currently calls picotool against USB, so it is not an offline preflight. Server deployment is a manual runbook / `cargo install`, not a protected entry. Searches of the project/ancestor AGENTS locations found none; this entry is newly introduced, not a modification of an imaginary existing policy.

## Change and state gate

A change ID records cause, introduction and escape causes, responsibility, deliverables, dependencies, stage evidence and explicit unapplied stages. Allowed stages are not_started → in_progress → verified, independently for design/implementation/independent_verification/operational_application/effectiveness_confirmation. `registered` never means implemented. Parent ownership is mandatory and the worker cannot self-assert independent acceptance or final closure.

1. Register/adjust domain contract and state tables; commit the specification before implementation.
2. Generate only declared values/state representations. Reject drift and forbidden dependency/write/construction paths before tests.
3. Implement within aggregate APIs; adapters retain IO ownership and bounded behavior.
4. Run root CLI and the same CI check locally. Compile-fail C/Rust and Python AST negatives must reject forbidden substitutions/state writes/unverified completion; historical faulty logic and partial records must fail the acceptance path. These are safeguards, not the primary model.
5. Bind gate receipts to current protected input hashes. Stage artifacts with SHA/provenance and independent parent approval. Offline flash preflight runs before any picotool command. No automatic firmware writing/service install is added.
6. Apply the entry/route only through a bounded integration preserving dirty user files; record active path and proof of rejection there. External branch protection / OS enforcement remain unapplied until separately authorized.

## Bypasses and completion

Direct `cmake`, `cargo`, `picotool load`, shell file copy, edits to the gate or arbitrary casts/reflection can bypass software conventions. Gate validation is fail-closed on the supported path, not an OS security boundary. C struct types and Rust private fields prohibit ordinary substitution; Python needs static adapter restrictions plus runtime validation of untrusted data. The specification can still be wrong. Physical timing, RF and actual device behavior remain runtime evidence.

Future work starts at the recorded active work root and uses this entry for both parent/worker. Existing accepted artifacts are retained; new enforcement does not retrospectively label them faulty. Independent acceptance and parent closure remain pending until their real evidence is added.
