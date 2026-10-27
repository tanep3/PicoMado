//! Optional LCD phrase example: a browser-authored card saved through plugin commands.
use crate::content::{ButtonAction, ContentError, ContentPlugin, PluginContext, PluginPlan, PluginPosition};
use crate::{Candidate, Capability, Event, Plugin, WorldQuery};
use std::collections::BTreeSet;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::plugin_jobs::{CommandContext, PluginWorkPlan, RefreshTargets};

const DEFAULT_PNG: &[u8] = include_bytes!("../assets/word-card.png");
const MAX_PNG: usize = 100_000;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Card { words: String, png: String }

fn decode_card(card: &Card) -> Result<Vec<u8>, ContentError> {
    if card.words.trim().is_empty() || card.words.len() > 160 ||
        card.words.chars().count() > 40 || card.words.chars().any(char::is_control) ||
        card.png.len() > (MAX_PNG + 2) / 3 * 4 {
        return Err(ContentError::InvalidSource);
    }
    let png = STANDARD.decode(&card.png).map_err(|_| ContentError::InvalidSource)?;
    if png.len() > MAX_PNG || png.len() < 24 || &png[..8] != b"\x89PNG\r\n\x1a\n" ||
        &png[12..16] != b"IHDR" || u32::from_be_bytes(png[16..20].try_into().unwrap()) != 160 ||
        u32::from_be_bytes(png[20..24].try_into().unwrap()) != 80 {
        return Err(ContentError::InvalidSource);
    }
    let image = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
        .map_err(|_| ContentError::InvalidSource)?.to_rgb8();
    if image.dimensions() != (160, 80) { return Err(ContentError::InvalidSource); }
    let mut frame = Vec::with_capacity(160 * 80 * 2);
    for pixel in image.pixels() {
        let [r, g, b] = pixel.0;
        let color = ((u16::from(r) >> 3) << 11) | ((u16::from(g) >> 2) << 5) | (u16::from(b) >> 3);
        frame.extend_from_slice(&color.to_be_bytes());
    }
    Ok(frame)
}

pub struct WordRules;
impl Plugin for WordRules {
    fn id(&self) -> &str { "my-words" }
    fn supports(&self, caps: &BTreeSet<Capability>) -> bool { caps.contains("lcd") }
    fn candidates<'a>(&'a self, _: &'a dyn WorldQuery, _: &'a Event) -> Box<dyn Iterator<Item = Candidate> + 'a> {
        Box::new(std::iter::empty())
    }
}

#[derive(Clone)]
pub struct WordContent { card: Card, frame: Vec<u8> }
impl Default for WordContent {
    fn default() -> Self {
        let card = Card { words: "おはよう".into(), png: STANDARD.encode(DEFAULT_PNG) };
        let frame = decode_card(&card).expect("bundled word card");
        Self { card, frame }
    }
}
impl ContentPlugin for WordContent {
    fn clone_box(&self) -> Box<dyn ContentPlugin> { Box::new(self.clone()) }
    fn id(&self) -> &'static str { "my-words" }
    fn publish(&mut self, _: &PluginContext, _: &[u8], _: &str, _: &str) -> Result<(), ContentError> { Err(ContentError::Unsupported) }
    fn position(&self, _: &PluginContext) -> Option<PluginPosition> { None }
    fn plan(&mut self, _: &PluginContext, _: ButtonAction) -> Result<PluginPlan, ContentError> { Err(ContentError::Unsupported) }
    fn current_frame(&self, _: &PluginContext) -> Option<Vec<u8>> { Some(self.frame.clone()) }
    fn web_state(&self) -> Value { json!({"words": self.card.words, "png": self.card.png}) }
    fn save_state(&self) -> Value { serde_json::to_value(&self.card).expect("card serialization") }
    fn load_state(&mut self, state: Value) -> Result<(), ContentError> {
        if state.is_null() { return Ok(()); }
        let card: Card = serde_json::from_value(state).map_err(|_| ContentError::InvalidSource)?;
        let frame = decode_card(&card)?;
        self.card = card; self.frame = frame; Ok(())
    }
    fn plan_command(&mut self, _: &CommandContext, value: &Value) -> Result<PluginWorkPlan, ContentError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Command { kind: String, words: String, png: String }
        let command: Command = serde_json::from_value(value.clone()).map_err(|_| ContentError::InvalidSource)?;
        if command.kind != "save" { return Err(ContentError::Unsupported); }
        let card = Card { words: command.words, png: command.png };
        let frame = decode_card(&card)?;
        self.card = card; self.frame = frame;
        Ok(PluginWorkPlan { refresh: RefreshTargets::Selected, ..Default::default() })
    }
    fn session_reset(&mut self, _: &str, _: u64) {}
    fn delete_device(&mut self, _: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::App;
    use crate::Effect;
    use std::sync::Arc;
    use crate::fetch::SystemClock;

    #[test]
    fn registered_phrase_reaches_server_send_effect() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::open_with_plugin_set(&temp.path().join("state.db"), temp.path(),
            Arc::new(SystemClock), crate::plugins::builtins()).unwrap();
        let token = app.hello("desk", BTreeSet::from(["lcd".into(), "buttons4".into()]), 1).unwrap();
        for _ in 0..5 { app.button(&token, ButtonAction::Next).unwrap(); }
        assert_eq!(app.view().devices[0].selected.as_deref(), Some("my-words"));
        let frames: Vec<_> = app.pending_effects().into_iter().filter_map(|(_, effect)| match effect {
            Effect::SendFrame { asset_handle, .. } => app.frame_asset(&asset_handle),
            _ => None,
        }).collect();
        assert!(frames.iter().any(|frame| frame.as_ref() == &WordContent::default().frame));
        assert_eq!(WordContent::default().frame.len(), 25_600);
    }
    fn test_png() -> String {
        let image = image::RgbImage::from_pixel(160, 80, image::Rgb([248, 64, 24]));
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image).write_to(&mut out, image::ImageFormat::Png).unwrap();
        STANDARD.encode(out.into_inner())
    }

    #[test]
    fn web_command_persists_and_refreshes_selected_lcd() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut app = App::open_with_plugin_set(&path, temp.path(), Arc::new(SystemClock), crate::plugins::builtins()).unwrap();
        let token = app.hello("desk", BTreeSet::from(["lcd".into(), "buttons4".into()]), 1).unwrap();
        for _ in 0..5 { app.button(&token, ButtonAction::Next).unwrap(); }
        let revision = app.plugin_web_view("my-words").unwrap().revision;
        let png = test_png();
        let command = json!({"kind":"save", "words":"新しい言葉", "png":png});
        let view = app.plugin_command("my-words", revision, command.clone()).unwrap();
        assert_eq!(view.data["words"], "新しい言葉");
        assert!(app.plugin_command("my-words", revision, command).is_err());
        let expected = decode_card(&Card { words: "新しい言葉".into(), png }).unwrap();
        let frames: Vec<_> = app.pending_effects().into_iter().filter_map(|(_, effect)| match effect {
            Effect::SendFrame { asset_handle, .. } => app.frame_asset(&asset_handle), _ => None
        }).collect();
        assert!(frames.iter().any(|frame| frame.as_ref() == &expected));
        drop(app);
        let app = App::open_with_plugin_set(&path, temp.path(), Arc::new(SystemClock), crate::plugins::builtins()).unwrap();
        let view = app.plugin_web_view("my-words").unwrap();
        assert_eq!(view.data["words"], "新しい言葉");
        assert_eq!(view.data["png"], STANDARD.encode(test_png_bytes()));
    }

    fn test_png_bytes() -> Vec<u8> { STANDARD.decode(test_png()).unwrap() }

    #[test]
    fn reject_invalid_words_and_images() {
        let png = test_png();
        for words in ["", " ", "hello\nworld", &"a".repeat(41)] {
            assert!(decode_card(&Card { words: words.into(), png: png.clone() }).is_err());
        }
        assert!(decode_card(&Card { words: "hello".into(), png: "invalid".into() }).is_err());
        assert!(decode_card(&Card { words: "hello".into(), png: "A".repeat(140_000) }).is_err());
        let mut wrong = test_png_bytes(); wrong[19] = 159;
        assert!(decode_card(&Card { words: "hello".into(), png: STANDARD.encode(wrong) }).is_err());
    }

}
