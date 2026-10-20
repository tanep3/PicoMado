//! Built-in plugin composition. Add implementations here without changing the
//! application, transport, or transition engine.

use std::collections::BTreeMap;

use crate::content::ContentPlugin;
use crate::{Engine, Registry};

pub const DEFAULT_ORDER: [&str; 5] = ["youtube", "caricature", "weather", "rain-radar", "clock"];

/// One registration owns matching rules and content. The application does not
/// add an ID branch when another plugin is registered.
pub struct PluginSet {
    pub(crate) engine: Engine,
    pub(crate) contents: BTreeMap<String, Box<dyn ContentPlugin>>,
    pub(crate) order: Vec<String>,
    pub(crate) startup: BTreeMap<String, std::sync::Arc<dyn crate::plugin_jobs::PluginStartupExecutor>>,
}
impl Default for PluginSet {
    fn default() -> Self { Self { engine: Engine::new(Registry::default()), contents: BTreeMap::new(), order: Vec::new(), startup: BTreeMap::new() } }
}
impl PluginSet {
    pub fn register(&mut self, rules: Box<dyn crate::Plugin>, content: Box<dyn ContentPlugin>) -> Result<(), crate::Error> {
        let id = rules.id().to_owned();
        if id != content.id() || id.is_empty() { return Err(crate::Error::UnknownPlugin(id)); }
        self.engine.registry.register(rules)?;
        self.contents.insert(id.clone(), content);
        self.order.push(id);
        Ok(())
    }
    pub fn register_startup(
        &mut self,
        id: &str,
        executor: std::sync::Arc<dyn crate::plugin_jobs::PluginStartupExecutor>,
    ) -> Result<(), crate::Error> {
        if !self.contents.contains_key(id) { return Err(crate::Error::UnknownPlugin(id.into())); }
        if self.startup.contains_key(id) { return Err(crate::Error::DuplicatePlugin(id.into())); }
        self.startup.insert(id.into(), executor);
        Ok(())
    }
}
pub fn builtins() -> PluginSet {
    let mut set=PluginSet::default();
    set.register(Box::new(crate::youtube::YoutubePlugin), Box::new(crate::plugin_adapters::YoutubeManaged::default())).expect("unique plugin");
    set.register(Box::new(crate::caricature::CaricaturePlugin), Box::new(caricature_content())).expect("unique plugin");
    set.register_startup("caricature", std::sync::Arc::new(crate::caricature::retention::RetentionRecovery)).expect("registered plugin");
    set.register(Box::new(crate::weather::WeatherPlugin), Box::new(crate::weather::WeatherContent::default())).expect("unique plugin");
    set.register(Box::new(crate::radar::RadarPlugin), Box::new(crate::radar::RadarContent::default())).expect("unique plugin");
    set.register(Box::new(crate::clock::ClockPlugin), Box::new(crate::clock::ClockContent::default())).expect("unique plugin");
    debug_assert_eq!(set.order, DEFAULT_ORDER);
    set
}
pub fn engine() -> Engine { builtins().engine }
pub fn contents() -> BTreeMap<String, Box<dyn ContentPlugin>> { builtins().contents }

/// Built-in source adapters live beside plugin registration. The generic fetch
/// runner does not need to know which plugins use HTTP.
pub async fn fetch(job: &crate::content::FetchJob) -> Result<Vec<u8>, String> {
    match job.plugin.as_str() {
        "weather" => crate::weather::JmaHttpFetcher::fetch_async(job).await,
        "rain-radar" => crate::radar::JmaRadarFetcher::fetch_async(job).await,
        _ => Err("unsupported plugin source".into()),
    }
}

/// External adapters are wired only at this composition boundary.
pub fn executors() -> crate::workers::Executors {
    // Reuse the existing interpreter read-only, including pepedpid/Pillow.
    let caricature = crate::caricature::executor::CaricatureExecutor {
        python: "/home/tane/dev/PicoW/PicoW_E-ink_Display/.venv/bin/python".into(),
        ..Default::default()
    };
    BTreeMap::from([
        (
            "youtube".into(),
            std::sync::Arc::new(crate::plugin_adapters::YoutubeExecutor)
                as std::sync::Arc<dyn crate::plugin_jobs::PluginJobExecutor>,
        ),
        (
            "caricature".into(),
            std::sync::Arc::new(caricature)
                as std::sync::Arc<dyn crate::plugin_jobs::PluginJobExecutor>,
        ),
    ])
}

fn caricature_content() -> crate::caricature::CaricatureContent {
    #[derive(serde::Deserialize)]
    struct Catalog {
        topics: Vec<crate::caricature::Selection>,
        styles: Vec<crate::caricature::Selection>,
    }
    let catalog: Catalog = serde_json::from_str(include_str!("../assets/caricature-catalog.json"))
        .expect("validated bundled catalog");
    let mut content = crate::caricature::CaricatureContent::default();
    content
        .set_catalog(catalog.topics, catalog.styles)
        .expect("validated bundled catalog");
    content
}
