# LCD-FIXED-UPDATE-20261027: fixed firmware operation

Scope: USB update of PicoMado-lcd1 with the independently assessed normal LCD UF2 built at 17e231ffa9db9b9cc9d93ecd55444eae0b7f26b1. PUBLIC-DOCS-20261027 remains incomplete. This operation cannot authorize publication or server deployment.

Admission must bind candidate, artifact, configuration, board ID, profile, build receipt and original check evidence to the immutable source commit. Validate historical provenance against an isolated read-only source snapshot, rather than pretending current gate changes were firmware build inputs. No alternate clone or worktree is required: a temporary git archive is sufficient for validation only.

The operation record must carry an expiry and explicit parent acceptance of a distinct independent review of the changed gate implementation. That review must bind the gate implementation commit and operation record hash. Parent acceptance of the existing firmware assessment is necessary but does not constitute gate implementation acceptance. Expired, revoked, unaccepted or stale approval must refuse before USB access.

Normal ship admission remains unchanged. The flash entry may select the narrowly scoped firmware operation only explicitly, and must retain manifest/profile checks and unique BOOTSEL board identity verification. The operation cannot accept a server artifact or substitute firmware from current HEAD.

Tests: valid fixed candidate; wrong UF2/device/board/config/profile; receipt/hash mutation; missing firmware review acceptance; missing independent gate review; changed implementation; expired/revoked approval; unchanged ordinary ship rejection. Fixtures use temporary files and no physical devices.

Stages: design in progress; implementation, independent verification, operational application and effectiveness confirmation not started. Parent owns final acceptance and closure. Firmware must remain unwritten until independent implementation review and parent acceptance.
