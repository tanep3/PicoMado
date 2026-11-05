# YOUTUBE-THUMBNAIL-20261105

Status: implemented on the repository's main working tree; automated verification complete. Server update and scoped live cleanup completed; physical display unverified.

## Cause and scope

The Web history renders one thumbnail per YouTube video record. Deleting the last converted variant previously removed only its frame directory; the remaining source record kept `thumbnail.jpg` and thumbnail metadata even though no playable conversion remained. Deleting an entire video already removes its whole directory.

## Contract

For a conversion deletion, keep the thumbnail while any other conversion remains. When the last conversion is deleted, remove `thumbnail.jpg` from that video's cache and clear its metadata fields. Keep source files and the record so the user can convert again. A failed deletion leaves the record available for retry. Full-video deletion and thumbnail retry retain their existing behavior.

## Verification

Extend the scoped deletion test to delete several conversions in sequence, check the thumbnail is retained until the last deletion, then check both the disk leaf and stored metadata are gone while source files remain. Confirm the adapter marks only the final conversion deletion for thumbnail cleanup. No existing user cache is swept or deleted by this change.

The scoped deletion test and adapter test passed. The default Rust library suite passed 286 tests and the all-feature suite passed 320 tests; one opt-in reference MP4 test is ignored by default and passed when run explicitly. The repository-wide gate remains blocked by the unrelated firmware flash refusal fixture noted in LOCAL-VIDEO-20261105.

## Follow-up: whole-video history cleanup

The full-video deletion command already removes the video record and its complete cache directory, but Web history exposed only source and conversion deletion. The latter deliberately retains the record for re-conversion, so records without playable conversions remained visible. An explicit per-video whole-history-and-cache deletion control now uses the existing confirmation and job path. Scoped deletion semantics remain distinct. A read-only inspection of the running server found 11 YouTube history records, including 2 with no conversions and thumbnails; all 11 cache directories corresponded to records and there were no unreferenced video directories.

With separate user authorization, the server was stopped and the complete configuration, SQLite DB, plugin assets and previous binary were backed up to `/home/tane/.local/share/PicoMado/backups/20261105-before-youtube-history-clean`. The new release binary was installed and the service restarted. The two empty records (`BaEh2IjHCJg`, `yao9ww00ul4`) were removed one at a time through the existing confirmed full-video API. Afterward, 9 history records and 9 matching cache directories remained, with no empty-conversion record. The new Web control returned HTTP 200 and the service remained active. No firmware or SSH operation was performed. Physical display behavior was not checked.
