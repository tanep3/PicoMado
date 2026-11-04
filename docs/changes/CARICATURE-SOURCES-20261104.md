# CARICATURE-SOURCES-20261104

Status: implemented and deployed; multi-source effectiveness confirmation pending.

## Cause and comparison

The news result contract used one `source_url` string while the artwork already stores `source_urls` as a list. The configured prompt and JSON schema did not require the string to contain exactly one URL. After the `future_past` topic began on 2026-11-02, the model frequently returned several sources joined by semicolons or newlines. `source_url_key` rejects whitespace, so retries exhausted before image generation. In the retained work records, earlier topics through 2026-11-01 returned single URLs; the topic shift exposed the latent mismatch. In the last 30 hourly jobs examined at 2026-11-04 00:00 UTC, 15 were validated and 15 remained at news research after three attempts. This supports the input-shape cause, not a claim that every earlier generation succeeded end to end.

## Contract

- Accept a legacy single `source_url`, a legacy semicolon/newline separated `source_url`, or a `source_urls` array. Normalize each into a bounded list without dropping sources. Reject empty segments, an absent source, or both fields together.
- Validate every URL using the existing syntax, port, DNS/public-address and 24-hour-reuse rules; reject duplicate canonical URLs within the result. Never treat a concatenation as one URL or accept just its first member.
- Store every validated URL separately in artwork metadata and repeat the reuse check after image creation. Preserve deserialization of saved jobs with the legacy single field.
- The existing JSON output schema and user-edited prompt files may continue to ask for `source_url`; decoding must work without rewriting those files or changing in-flight prompt snapshots.

## Verification and application

- Unit tests: single/multiple decoding, malformed and unsafe URLs, duplicate/used sources, and saved-job compatibility.
- Integration test: fake CLI returns two sources; generated artwork retains both, while a saved legacy single-source job resumes.
- `CARGO_TARGET_DIR=/tmp/picomado-source-target cargo test --offline --manifest-path service/Cargo.toml --lib caricature::`: 56 passed.
- Full `--lib --quiet` suite: 282 passed with local loopback test sockets allowed. An initial sandboxed run had 267 passed and 15 `Operation not permitted` failures at `127.0.0.1` binds; the rerun passed all 282 without code changes.
- `git diff --check`: passed. The repository-wide `cargo fmt --check` reports extensive pre-existing formatting drift outside the changed files and is not used as a change-specific gate.
- Operational application and live generation remain pending unless separately performed and verified; a code commit alone is not proof of production behavior.

## Production application, 2026-11-04

The installed server SHA256 before this change was `a48052af6f99a42ee277b7aa27926cea4c1a6a1a8b9784389de18a406f963604`, documented as the build of `b2785e4`. To avoid deploying later unrelated server changes from `main`, release commit `315a393fa8901be19802e9671dfcf5d5645fee65` applies only the two changed caricature source files from `aa7da51` to that running baseline. Its patch SHA256 is `235b58a3d79e4a847c628bc1773f185c5d024b9e8a86940df4bd8ca4e57044f6`. The release worktree passed 282 library tests with `caricature-retention-delete` enabled. A release binary was built offline with that feature preserved, and installed SHA256 is `4d2a75b0e3b7ec4717b8abbe808798a4d089b33152689058cf44fd0d86eafe14`.

Before stopping, the application had no nonterminal caricature or common jobs; the service cgroup contained only `picomado-server`. The existing binary was backed up, then the service was stopped and the configuration, DB/WAL and plugin data were archived together. A consistent SQLite backup passed `integrity_check`. The new binary was installed and the service restarted. The service is active with zero restarts; GET `/api/state` succeeded. Settings, order revision, plugin order, and each device's selection and enabled state matched the pre-stop snapshot. The private rollback bundle is under the 2026-11-04 task's `picomado-caricature-rollout-20261104/backup` directory and must not be committed or published.

One post-install manual generation using the existing `future_past` topic, `manual_9`, fetched news on its first attempt, drew on its first image attempt, and published `manual_9-1`. Its returned source was a single URL, so this proves the deployed generation route but does not yet prove multi-source publication in production. The three rejected pre-fix candidates for `hour_498264` each split into two or three individually well-formed, publicly resolving URLs. Added tests cover that delimiter shape, per-source validation and metadata preservation. A further manual generation was rejected by automatic approval review; it was not executed. The manual publication advanced the hourly freshness clock to 2026-11-04 03:41:43 UTC, making the next automatic attempt due no earlier than 04:41:43 UTC. Multi-source effectiveness remains to be observed on a subsequent generated result.
