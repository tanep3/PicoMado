# CARICATURE-SOURCES-20261104

Status: implementation verified locally; operational application and effectiveness confirmation pending.

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
