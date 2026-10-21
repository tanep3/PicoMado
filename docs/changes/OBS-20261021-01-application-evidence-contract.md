# OBS-20261021-01 / P2-04 — scoped route application evidence

Specification before the limited correction to `workflow.py` application/effectiveness admission. The original P2-01/02/03 remain accepted within their reviewed boundary. Source and device/service routes are not applied by this work.

## Meaning and ownership

An application declaration is not an effect. `AppliedRouteEvidence` binds the existing canonical active root, the reviewed source root/commit/subject, actual entry files and a successful execution through that root. `EffectivenessEvidence` additionally binds the exact accepted application declaration and both allowed and refused actual executions. Parent approval still owns these stages; a worker's local fixture is never operational application.

The pure shared workflow requires a scoped-evidence verifier for these stages, rejecting them when it is absent. One shared evidence adapter verifies both stage kinds, using the existing bounded SHA proof loader and the project's input snapshot adapter. It must reject nonexistent/symlink-ambiguous roots, wrong source/subject/entry bytes, missing receipt/log/input, wrong stage/root, contradictory result and assertion-only strings.

## Controlled evidence production and verification

A read-only route probe invokes the actual root CLI from the declared active root. It does not copy/install source, edit AGENTS, change permissions or apply a service/device. Before and after execution it checks the same subject and committed source. It retains command, cwd, entry/interpreter hashes, expected operation, return code, timestamps, saved result log and input SHA.

The fixed operations are a successful current-check verification and refusal of the same receipt with its subject digest invalidated. Application requires the positive execution; effectiveness requires both and a hash reference to the exact application proof already declared in the ledger. The current source/active entry are re-read during every admission; stored assertions alone do not qualify. Logs are compared with receipt declarations and expected results, not merely required as nonempty strings.

The supported initial application model selects a complete existing development worktree as the active root; its protected subject and AGENTS/root gate/standard entry must match the reviewed source. A partial redirect/wrapper deployment is not silently treated as this model. It needs its own contract if later selected.

Local trusted receipts are not a cryptographic proof against an operator able to rewrite all code and evidence. This limitation does not permit missing roots or unexecuted/string-only evidence on the normal path.

## Required verification

Replay the exact nonexistent-root/plain-refusal-string counterexample and refuse close. Produce a real probe in a private test worktree, then verify application and effectiveness through the common admission operation. Remove/change target, source, entry, command/result/log/input/proof hash or application linkage and refuse. Failed/unexecuted assertions cannot close. No original work route or hardware is used as the positive fixture.

This parent-authorized revision invalidates the prior local implementation proof for the new subject; its commits/receipts remain preserved. Design is fixed first, then this limited implementation, rejection evidence and local verification are fixed. Independent review/application/effectiveness in the actual project stay pending.
