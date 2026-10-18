# PicoMado user-service layout

`config.toml` is the non-secret server setting file installed at
`/home/tane/.config/PicoMado/config.toml`. The common transactional SQLite
database is `/home/tane/.config/PicoMado/data/picomado.db`; it currently holds
device records, settings, outbox state, and plugin snapshot metadata. This is
PicoMado's own database, separate from the older player service.

Each registered built-in plugin receives its own `data/` and `cache/` under
`/home/tane/.config/PicoMado/<plugin>/`:

| Plugin | Durable content directory | Replaceable fetch/render cache |
| --- | --- | --- |
| YouTube | `youtube/data/` | `youtube/cache/` |
| Caricature | `caricature/data/` | `caricature/cache/` |
| Weather | `weather/data/` | `weather/cache/` |
| Rain radar | `rain-radar/data/` | `rain-radar/cache/` |
| Clock | `clock/data/` (settings in SQLite) | `clock/cache/` (unused) |

The current weather and radar adapters render in memory and keep their latest
transactional snapshot in the common DB. No downloaded assets are written to
these plugin directories yet. Future plugin-owned files belong in their own
directory; no plugin artifact goes into a common cache folder. The server
creates missing directories and leaves existing files untouched. Logs go to
the user journal, not to the data tree.

`picomado.service` is the user unit template. Install the crate from
`service/` using the existing Cargo install root (`/home/tane`), then copy the
template config and unit only when their destinations are absent. The unit
uses `/home/tane/bin/picomado-server` and reads the installed config.
