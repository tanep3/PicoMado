# OBS-20261021-01: three review corrections

This is the specification before correcting the retained implementation WIP. Scope is exactly P2-01/02/03 in `/tmp/picomado-process-handoff-review-un44g0yw/review-outcome.json`. No operational adoption, hardware, service, SSH or push.

## 1. Project state and evidence

All canonical documents and change declarations participate in protected inputs. Separate a subject digest (code, contracts and change intent) from a workflow digest (the complete stage/evidence declaration). The evidence digest combines them. A lifecycle update therefore changes the evidence digest without recursively changing the subject attested by an earlier check.

One project-independent workflow operation validates every declared stage and its proof on every root/build/preflight entry. Verified design requires a real prior commit containing the current declared design artifacts. Verified implementation requires an intact gate receipt for this subject; independent verification requires parent acceptance for this subject; application and effectiveness require their own scoped evidence after prerequisites. Empty evidence, wrong stage/subject, missing or altered proof and skipped prerequisites are rejected. A worker cannot close its own independent or operational stages. Legitimate later transitions may use older evidence whose subject is unchanged; the current workflow is always revalidated.

## 2. Domain completion

CaptureReport becomes an immutable, opaque domain result, not a dict subclass. Ordinary mapping operations (including |=, setdefault, update and base-dict methods) cannot modify it. Immutable nested state and explicit complete⇒verified-evidence invariants are checked by constructors and run_outcome. The shared evidence verifier remains the only capability issuer. Serialization to plain records is an adapter operation; passing that record back as a domain report is rejected. Adapter |= and success-field writes are static errors. Python reflection/patching and the ability to rewrite trusted verifier source remain outside this software trust boundary.

## 3. Build lineage

record no longer relabels an arbitrary supplied artifact as built from the checked source. The supported build entry executes the actual builder, snapshots the checked subject, actual source root, SDK HEAD/gitlinks/clean state, actual CMake cache/compile commands/flags and compiler identity (firmware), or Cargo manifest/lock/features/fingerprint/toolchain/environment (server), then verifies unchanged inputs on successful completion and binds the resulting artifact hash. record and preflight recheck this build receipt and its referenced inputs. No import of an old accepted artifact is fabricated.

The existing firmware build script delegates its build step to this controlled operation; server builds use the same gate API. Artifact/SDK/flags/config/source changes, missing builder receipt and failed or unrelated builds fail before USB. The receipt is a local trusted-build record, not a cryptographic defense against an operator able to rewrite all scripts/receipts/toolchains; external protection remains unapplied.

## Fixed refusal conditions

- Change the canonical contract: subject/evidence digest changes and old proof cannot stage. Change verified stage with no proof: evidence digest changes and the common workflow refuses every entry. A valid phase transition with old same-subject evidence does not require circular self-hashing.
- Replay `incomplete(...); report |= {complete: True}`: TypeError. Its static adapter equivalent is refused. A plain mapping or a report without verified evidence cannot produce successful RunOutcome.
- Supply the existing synthetic UF2 with only a source receipt: record refuses. Run a real isolated build as positive control; then change source root, SDK/gitlink, compiler/flags, config, builder receipt or artifact: record/preflight refuses. No device is invoked for these tests.

Design is fixed first; implementation and rejection evidence are separately committed. Independent verification, operational application and effectiveness remain pending parent ownership.
