# Minimal development standard v1

This package is project independent. Project language, values, invariants and adapters belong to the project contract, not this standard. It preserves Query → pure rule/transition → external effect and single responsibility. File splitting alone is insufficient.

1. One ubiquitous language names distinct facts, units and identities. Value objects prohibit accidental substitution.
2. A single aggregate owns mutable lifecycle and completion. Adapters decode/encode or perform IO; orchestration orders effects. Neither builds a successful domain result from unchecked facts.
3. Dependencies point from adapters/orchestration toward pure domain and data, never from domain to USB, filesystem, subprocess or concrete plugin.
4. Fix contract/state tables before implementation. Small generated representations preserve that model; generation is not the design authority or a general framework.
5. Root gate checks generated drift, dependency boundaries, restricted writes/result construction, and required change evidence. Tests supplement these structural restrictions.
6. Build/ship reports bind source, gate version, artifact and approval. No receipt authorizes physical execution. Direct tool calls remain bypasses unless external permissions/repository protections enforce the approved path.
7. Keep design, implementation, independent verification, operational application and effectiveness separate. The parent closes a change only on attached evidence for every stage. No zero-bug claim.

## Adoption in another project

Copy/version this small directory and generic checker, set a project-specific boundary configuration, add an AGENTS link to this exact version, register the first change, and wire the actual root/build/ship paths. Review local rules and run negative structural fixtures before setting `operational_application=applied`. Do not bulk-edit unrelated repositories. The tracked copy in this repository is the current shareable source; future projects pin its commit/hash rather than a mutable task conversation.
