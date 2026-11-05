# YOUTUBE-THUMBNAIL-20261105

Status: implemented on the repository's main working tree; automated verification complete. Operational application pending.

## Cause and scope

The Web history renders one thumbnail per YouTube video record. Deleting the last converted variant previously removed only its frame directory; the remaining source record kept `thumbnail.jpg` and thumbnail metadata even though no playable conversion remained. Deleting an entire video already removes its whole directory.

## Contract

For a conversion deletion, keep the thumbnail while any other conversion remains. When the last conversion is deleted, remove `thumbnail.jpg` from that video's cache and clear its metadata fields. Keep source files and the record so the user can convert again. A failed deletion leaves the record available for retry. Full-video deletion and thumbnail retry retain their existing behavior.

## Verification

Extend the scoped deletion test to delete several conversions in sequence, check the thumbnail is retained until the last deletion, then check both the disk leaf and stored metadata are gone while source files remain. Confirm the adapter marks only the final conversion deletion for thumbnail cleanup. No existing user cache is swept or deleted by this change.

The scoped deletion test and adapter test passed. The default Rust library suite passed 286 tests and the all-feature suite passed 320 tests; one opt-in reference MP4 test is ignored by default and passed when run explicitly. The repository-wide gate remains blocked by the unrelated firmware flash refusal fixture noted in LOCAL-VIDEO-20261105.
