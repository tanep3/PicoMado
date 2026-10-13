//! Built-in plugin composition. Add implementations here without changing the
//! application, transport, or transition engine.

use std::collections::{BTreeMap, BTreeSet};

use crate::content::ContentPlugin;
use crate::{Capability, Engine, Event, Plugin, Registry, WorldQuery};

pub const DEFAULT_ORDER: [&str; 4] = ["youtube", "caricature", "weather", "rain-radar"];

struct SkeletonPlugin(&'static str);

impl Plugin for SkeletonPlugin {
    fn id(&self) -> &str {
        self.0
    }
    fn supports(&self, capabilities: &BTreeSet<Capability>) -> bool {
        capabilities.contains("lcd") || capabilities.contains("eink")
    }
    fn candidates<'a>(
        &'a self,
        _query: &'a dyn WorldQuery,
        _event: &'a Event,
    ) -> Box<dyn Iterator<Item = crate::Candidate> + 'a> {
        Box::new(std::iter::empty())
    }
}

pub fn engine() -> Engine {
    let mut registry = Registry::default();
    registry
        .register(Box::new(SkeletonPlugin("youtube")))
        .expect("unique plugin");
    registry
        .register(Box::new(SkeletonPlugin("caricature")))
        .expect("unique plugin");
    registry
        .register(Box::new(crate::weather::WeatherPlugin))
        .expect("unique plugin");
    registry
        .register(Box::new(crate::radar::RadarPlugin))
        .expect("unique plugin");
    Engine::new(registry)
}

pub fn contents() -> BTreeMap<String, Box<dyn ContentPlugin>> {
    BTreeMap::from([
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
