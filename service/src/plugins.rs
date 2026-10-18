//! Built-in plugin composition. Add implementations here without changing the
//! application, transport, or transition engine.

use std::collections::BTreeMap;

use crate::content::ContentPlugin;
use crate::{Engine, Registry};

pub const DEFAULT_ORDER: [&str; 5] = ["youtube", "caricature", "weather", "rain-radar", "clock"];

pub fn engine() -> Engine {
    let mut registry = Registry::default();
    registry
        .register(Box::new(crate::youtube::YoutubePlugin))
        .expect("unique plugin");
    registry
        .register(Box::new(crate::caricature::CaricaturePlugin))
        .expect("unique plugin");
    registry
        .register(Box::new(crate::weather::WeatherPlugin))
        .expect("unique plugin");
    registry
        .register(Box::new(crate::radar::RadarPlugin))
        .expect("unique plugin");
    registry
        .register(Box::new(crate::clock::ClockPlugin))
        .expect("unique plugin");
    Engine::new(registry)
}

pub fn contents() -> BTreeMap<String, Box<dyn ContentPlugin>> {
    BTreeMap::from([
        (
            "clock".into(),
            Box::new(crate::clock::ClockContent::default()) as Box<dyn ContentPlugin>,
        ),
        (
            "youtube".into(),
            Box::new(crate::plugin_adapters::YoutubeManaged::default()) as Box<dyn ContentPlugin>,
        ),
        (
            "caricature".into(),
            Box::new(caricature_content()) as Box<dyn ContentPlugin>,
        ),
        (
            "weather".into(),
            Box::new(crate::weather::WeatherContent::default()) as Box<dyn ContentPlugin>,
        ),
        (
            "rain-radar".into(),
            Box::new(crate::radar::RadarContent::default()) as Box<dyn ContentPlugin>,
        ),
    ])
}

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
