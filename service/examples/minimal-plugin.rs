//! Small LCD-only plugin skeleton. It is deliberately not registered in builtins().
//! Run its test with `cargo test --offline --example minimal-plugin` in an isolated test target.
use picomado_core::content::{ButtonAction, ContentError, ContentPlugin, PluginContext, PluginPlan, PluginPosition};
use picomado_core::plugins::PluginSet;
use picomado_core::{Candidate, Capability, Event, Plugin, WorldQuery};
use std::collections::BTreeSet;

struct SolidRules;
impl Plugin for SolidRules {
    fn id(&self) -> &str { "solid-example" }
    fn supports(&self, caps: &BTreeSet<Capability>) -> bool { caps.contains("lcd") }
    fn candidates<'a>(&'a self, _: &'a dyn WorldQuery, _: &'a Event) -> Box<dyn Iterator<Item = Candidate> + 'a> {
        Box::new(std::iter::empty())
    }
}

#[derive(Clone, Default)]
struct SolidContent;
impl ContentPlugin for SolidContent {
    fn clone_box(&self) -> Box<dyn ContentPlugin> { Box::new(self.clone()) }
    fn id(&self) -> &'static str { "solid-example" }
    fn publish(&mut self, _: &PluginContext, _: &[u8], _: &str, _: &str) -> Result<(), ContentError> {
        Err(ContentError::Unsupported) // No external source or saved asset.
    }
    fn position(&self, _: &PluginContext) -> Option<PluginPosition> { None }
    fn plan(&mut self, _: &PluginContext, _: ButtonAction) -> Result<PluginPlan, ContentError> {
        Err(ContentError::Unsupported) // Add button behavior only after defining its rule.
    }
    fn current_frame(&self, _: &PluginContext) -> Option<Vec<u8>> {
        Some(vec![0; 160 * 80 * 2]) // Black RGB565BE pixels, exact LCD frame length.
    }
    fn session_reset(&mut self, _: &str, _: u64) {}
    fn delete_device(&mut self, _: &str) {}
}

fn main() {
    let mut plugins = PluginSet::default();
    plugins.register(Box::new(SolidRules), Box::new(SolidContent)).expect("matching unique IDs");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registers_matching_id_and_keeps_lcd_size() {
        let mut plugins = PluginSet::default();
        plugins.register(Box::new(SolidRules), Box::new(SolidContent)).unwrap();
        assert!(plugins.register(Box::new(SolidRules), Box::new(SolidContent)).is_err());
        let frame = SolidContent.current_frame(&dummy_context());
        assert_eq!(frame.unwrap().len(), 25_600);
        assert!(SolidRules.supports(&BTreeSet::from(["lcd".to_owned()])));
        assert!(!SolidRules.supports(&BTreeSet::from(["eink".to_owned()])));
    }
    fn dummy_context() -> PluginContext {
        use picomado_core::{Mode, SessionToken};
        PluginContext { token: SessionToken { device_id: "example".into(), incarnation: 1, session_epoch: 1 },
            settings_revision: 1, mode: Mode::Normal }
    }
}
