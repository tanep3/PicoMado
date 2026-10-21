# Development entry

All parent and worker tasks read [the versioned standard](development/standard/v1/standard.md), [development process](docs/development-process.md), and [domain contract](docs/contracts-observation-design.md#obs-20261021-01-observation-domain) before changing governed code.

Associate each change with an open ID in `docs/changes/`. Fix the domain model/contract first; generate representations; pass static boundaries; implement through aggregate APIs. `python3 tools/project_gate.py check` is the root gate. Never equate local tests, a document commit, or an isolated branch with operational completion. The parent owns final closure and independent approval.

No device/service/SSH/permission/push operations are authorized by this file. Test fixtures use temporary paths and fresh PTYs only. Direct build/flash/deploy commands bypassing the root gate are unsupported. See the process for known enforcement limits and the approved work root.
