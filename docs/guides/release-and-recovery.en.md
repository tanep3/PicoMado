# Updates, backups and recovery

[日本語](release-and-recovery.md) · [First setup](first-setup.en.md)

Before an update, save the current server executable, configuration, SQLite database, plugin `data/` and any required `cache/`, plus the previous UF2 and its receipt for the same board. Keep the files as a matched set in a private location. Do not copy only the database file while SQLite WAL is active: stop the service with the owner's approval or use a SQLite-supported consistent backup. Do not overwrite an existing configuration or move data automatically.

Keep the matching source tree, SDK, check report and logs, complete build directories and their sibling configure/build logs, receipts and artifact SHA256 values with the backup. Preflight records absolute paths: restore those files to their original locations before checking an old build. Those local files may contain device identifiers and paths; do not publish them. Follow [first setup](first-setup.en.md) to build and run preflight before updating the server and device separately. A passing check does not prove that an update or physical display works.

If an update fails, record the symptoms and the candidate's receipt. Restore a matching old executable, configuration, database and plugin data: an old executable alone may not understand a newer database schema. For firmware, verify the old receipt, UF2 and Flash ID for the intended board before the owner writes it. After recovery, check startup, connectivity, display and buttons independently.
