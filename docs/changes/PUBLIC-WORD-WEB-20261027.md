# PUBLIC-WORD-WEB-20261027 — editable words end to end

## Problem and comparison

The earlier word tutorial compiled an image into the server and exposed only a UI label. It did not let a person edit words in the Web UI. Existing managed plugins instead own a revisioned web state and command, durable state, and refresh targets; the host persists the planned state and sends selected devices their new frame. Use that existing path for this example. The opt-in feature and five-plugin default remain unchanged.

## Contract

The Web UI displays and edits one shared short phrase. Its browser renders a 160×80 preview using an available Japanese font, validates fit, and submits the phrase and the bounded PNG together with the existing plugin revision. Show the preview so missing glyphs or font differences are visible before saving. User text enters DOM through input values or textContent, never HTML. Empty, control-containing, excessively long text, malformed/oversized PNG and images of other dimensions are rejected. The backend treats the supplied image as a user-authored image; it cannot independently prove that the browser drew the claimed text. No server font installation or per-edit rebuild is required.

The plugin owns the phrase and accepted image as one durable state and converts pixels to 160×80 RGB565BE. Its command operates on a clone and returns Selected refresh through the existing aggregate; persistence precedes publication and delivery. Stale revision is rejected, offline devices receive the saved content on selection or reconnect, and older installations without saved state retain a usable default. The UI must not appear when the feature is absent. Existing database contents and other plugins are preserved.

## Verification

Test invalid text, malformed and oversized images, dimension rejection, stale revisions, persistence and reload, default composition, selected device refresh and exact frame bytes. Exercise the UI on isolated localhost and deliver the edited frame to a simulated LCD through the existing WebSocket protocol. Test fixtures use temporary state and no production service or device. Physical display, independent parent acceptance and deployment remain separate.

## Review correction: draft and revision

A preview alone creates no draft. The first edit binds the draft to the displayed revision; polling may refresh the server view but cannot rebase an edited draft. Save uses the bound revision and preserves input on rejection. A successful save clears only the submitted draft. Browser control-character validation matches Rust Unicode control characters.
