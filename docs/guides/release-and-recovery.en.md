# Updates, backups and recovery

[日本語](release-and-recovery.md) · [First setup](first-setup.en.md)

Before updating, privately save the current server executable, configuration, SQLite database, plugin `data/` and any needed `cache/`, and the old UF2 as a matching set. Do not copy only the database file while SQLite WAL is active: stop the service or use a SQLite-supported consistent backup.

If an update fails, restore the matching old executable, configuration, database and plugin data; an old executable alone may not understand a newer database schema. To restore firmware, copy the old UF2 to the BOOTSEL drive. Check startup, connection, display and buttons independently after recovery.
