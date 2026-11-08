//! Built-in plugin composition. Add implementations here without changing the
//! application, transport, or transition engine.

use std::collections::BTreeMap;

use crate::content::ContentPlugin;
use crate::{Engine, Registry};

#[cfg(not(feature = "word-tutorial"))]
pub const DEFAULT_ORDER: [&str; 8] = ["youtube", "caricature", "weather", "rain-radar", "clock", "local-video", "local-image", "local-music"];
#[cfg(feature = "word-tutorial")]
pub const DEFAULT_ORDER: [&str; 9] = ["youtube", "caricature", "weather", "rain-radar", "clock", "local-video", "local-image", "local-music", "my-words"];

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
    set.register(Box::new(crate::local_video::LocalVideoPlugin), Box::new(crate::local_video::LocalVideoContent::default())).expect("unique plugin");
    set.register(Box::new(crate::local_image::LocalImagePlugin), Box::new(crate::local_image::LocalImageContent::default())).expect("unique plugin");
    set.register(Box::new(crate::local_music::LocalMusicPlugin), Box::new(crate::local_music::LocalMusicContent::default())).expect("unique plugin");
    #[cfg(feature = "word-tutorial")]
    set.register(Box::new(crate::word::WordRules), Box::new(crate::word::WordContent::default())).expect("unique plugin");
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
    // A user's existing local interpreter remains the default when present.
    // An explicit override lets a new installation use its own environment.
    let python = std::env::var_os("PICOMADO_CARICATURE_PYTHON")
        .map(std::path::PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .map(|home| home.join("dev/PicoW/PicoW_E-ink_Display/.venv/bin/python"))
                .filter(|path| path.is_file())
                .unwrap_or_else(|| "python3".into())
        });
    let caricature = crate::caricature::executor::CaricatureExecutor {
        python,
        ..Default::default()
    };
    BTreeMap::from([
        ("local-music".into(), std::sync::Arc::new(crate::local_music::LocalMusicExecutor) as std::sync::Arc<dyn crate::plugin_jobs::PluginJobExecutor>),
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
        (
            "local-video".into(),
            std::sync::Arc::new(crate::local_video::LocalVideoExecutor)
                as std::sync::Arc<dyn crate::plugin_jobs::PluginJobExecutor>,
        ),
        (
            "local-image".into(),
            std::sync::Arc::new(crate::local_image::LocalImageExecutor)
                as std::sync::Arc<dyn crate::plugin_jobs::PluginJobExecutor>,
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_jobs::{
        JobExecutionContext, JobOutcome, JobProgressSink, JobToken, PluginPaths,
    };

    #[tokio::test]
    async fn registered_local_image_executor_scans_uploads() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        std::fs::create_dir_all(data.join("imports")).unwrap();
        std::fs::write(data.join("imports/photo.png"), b"uploaded image").unwrap();
        let (_, cancel) = tokio::sync::watch::channel(false);
        let executor = executors()
            .remove("local-image")
            .expect("local image executor registered");
        let outcome = executor
            .execute(
                serde_json::json!({"kind":"scan"}),
                JobExecutionContext {
                    token: JobToken {
                        plugin: "local-image".into(),
                        slot: "scan".into(),
                        generation: 1,
                    },
                    paths: PluginPaths {
                        data,
                        cache: dir.path().join("cache"),
                    },
                    cancel,
                    progress: JobProgressSink::default(),
                },
            )
            .await;
        assert_eq!(outcome, JobOutcome::Succeeded(serde_json::json!(["photo.png"])));
    }
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

/// Invoke outside the application lock; caller owns stream/session fencing.
pub async fn read_audio_chunk(plugin: &str, spec: crate::plugin_jobs::AudioPlaybackSpec, offset: u64, max: u32, paths: crate::plugin_jobs::PluginPaths) -> Result<Vec<u8>, crate::plugin_jobs::JobFailure> {
    let executor = executors().remove(plugin).ok_or_else(|| crate::plugin_jobs::JobFailure::new("unsupported", "audio executor unavailable"))?;
    executor.read_audio(spec, offset, max, paths).await
}

/// State is owned by one socket transfer, never shared by asset or device ID.
pub async fn read_audio_session(handle: crate::plugin_jobs::AudioReadHandle, plugin: &str, spec: crate::plugin_jobs::AudioPlaybackSpec, offset: u64, max: u32, paths: crate::plugin_jobs::PluginPaths) -> Result<Vec<u8>, crate::plugin_jobs::JobFailure> {
    let mut state = handle.lock().await;
    let executor = executors().remove(plugin).ok_or_else(crate::plugin_jobs::JobFailure::unsupported)?;
    if !state.opened {
        state.reader = executor.open_audio(spec.clone(), paths.clone()).await?;
        state.opened = true;
    }
    if let Some(reader) = state.reader.as_mut() { reader.read(offset, max).await }
    else { executor.read_audio(spec, offset, max, paths).await }
}
