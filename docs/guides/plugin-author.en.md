# Start with a small plugin

[日本語](plugin-author.md) · [Project overview](../../README.md)

The [minimal example](../../service/examples/minimal-plugin.rs) produces a solid black LCD image without storage or network access. It is **not registered in the current server**, so it is a starting point for development rather than another built-in feature. Its test uses a temporary Cargo target and starts neither the service nor a physical device.

1. `Plugin` supplies the ID, supported device capability and pure candidate rules. `ContentPlugin` owns state, input planning and pixels. Both use the same ID.
2. Register the rule and content together with `PluginSet::register`. To include a new plugin in the server, update `builtins()` and `DEFAULT_ORDER` in `service/src/plugins.rs`, then record settings and persistence compatibility in the contract. The example's `main` only exercises the registration API.
3. If the plugin accepts input, define its behavior in `plan`. Keep external fetches and file IO out of pure planning; use the existing job and executor boundary. LCD `current_frame` uses 160×80 RGB565BE, or 25,600 bytes. E-ink needs its own format and capability checks.
4. Leave button events, generations and frame acknowledgements with the shared device framework. Do not infer support for a new screen or MCU from this example. Adding a server plugin also requires a change ID, contract, root gate and independent review.
