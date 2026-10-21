# P2-04 / scoped application evidence operation

The authority is [the contract fixed before implementation](OBS-20261021-01-application-evidence-contract.md). This note documents the limited adapter; it does not assert operational adoption.

After independent parent acceptance is recorded for the current subject, choose a complete existing clean worktree with the same protected subject as the reviewed source. The source's root CLI can then run a read-only probe:

```sh
python3 tools/project_gate.py probe-route \
  --active-root /absolute/existing/reviewed-worktree \
  --report /absolute/current/check.json \
  --out /absolute/fresh/evidence-directory
```

The command does not install a route. It executes that active worktree's real `verify-check` twice, using the current input and the same input with its subject invalidated. A fresh output directory retains both inputs, result logs, execution receipts and `probe.json`. It refuses stale check evidence, dirty/different roots, changed scope and contradictory execution results. A nonzero command does not confer acceptance; partial output must be retained for diagnosis. Never use fixture check metadata as project acceptance.

Parent application evidence has `schema: 1`, `change`, `stage: operational_application`, `subject_digest`, `owner: parent`, `applied: true`, canonical `source_root` / `active_root`, and `probe: {path, SHA256}`. The ledger's application stage references this proof as `{kind: application, path, SHA256}`. Parent effectiveness evidence uses the same scope and probe, `stage: effectiveness_confirmation`, `confirmed: true`, plus `application` equal to that exact declared application reference. Each proof is a saved JSON file, never an assertion string. Ordinary root admissions verify these through the same workflow operation; no special CLI exception grants completion.

Admission rechecks existing roots, protected subjects, entry bytes and their declared commits, interpreter bytes, both evidence hash chains, exact cwd/commands, input meaning and actual saved results. The shared workflow fails closed without its evidence verifier. Lifecycle-only ledger commits may follow the probe; protected subject and entry/source binding must still hold. Replacing/removing the active route or evidence invalidates admission. This supports the complete-worktree model only; redirect wrappers need a separately fixed contract.

Local receipts trust the controlled producer and parent authority. An operator who rewrites all source/evidence can bypass local conventions; these are not signatures or OS enforcement. The private fixture proves the supported admission path and denial behavior. It does not establish adoption, physical execution, CI protection or measured field effectiveness. Those project stages remain pending until independently accepted real evidence exists.
