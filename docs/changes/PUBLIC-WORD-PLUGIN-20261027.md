# PUBLIC-WORD-PLUGIN-20261027 — favorite words tutorial

## Contract

A user can render a short phrase including Japanese onto a 160×80 LCD card without an external API. The build embeds the PNG produced from an installed Japanese font. An opt-in `word-tutorial` server feature registers `my-words` with the existing PluginSet. Its rules advertise LCD only; content returns exactly 160×80 RGB565BE pixels. The default five-plugin server remains unchanged. A user can change phrase, size and position by regenerating the PNG and rebuilding.

The initial exercise provides a legible PNG preview, an isolated server-side selection and SendFrame test, and clear instructions for a separately checked server candidate and physical LCD verification. It does not claim E-ink compatibility or physical completion from the isolated test. The ordinary backend remains responsible for generation, transport and button events. Public guides describe actions for an unfamiliar reader and avoid private paths or credentials.

## Comparison and evidence

The previous minimal plugin returned a black frame and registered only in its example process; it could not show the user a phrase on a selected device. The bundled LCD bitmap font lacks common characters such as お, は, よ and う, so text must be rendered through a Japanese font before RGB565 conversion. Compare the opt-in plugin against the unchanged default composition. The acceptance test must select the plugin through App, observe a SendFrame with the exact rendered bytes, and keep all state in temporary paths. Physical drawing remains separate.
